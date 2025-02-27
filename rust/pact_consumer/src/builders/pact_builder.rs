use std::path::PathBuf;

use pact_models::{Consumer, Provider};
use pact_models::interaction::Interaction;
use pact_models::pact::Pact;
use pact_models::sync_pact::RequestResponsePact;
use pact_models::v4::async_message::AsynchronousMessage;
use pact_models::v4::pact::V4Pact;
use pact_models::v4::sync_message::SynchronousMessage;
use pact_plugin_driver::catalogue_manager;
use pact_plugin_driver::catalogue_manager::CatalogueEntryType;
use pact_plugin_driver::plugin_manager::{drop_plugin_access, load_plugin};
use pact_plugin_driver::plugin_models::{PluginDependency, PluginDependencyType};
use tracing::trace;

use pact_matching::metrics::{MetricEvent, send_metrics};

use crate::builders::message_builder::MessageInteractionBuilder;
use crate::builders::message_iter::{asynchronous_messages_iter, MessageIterator, synchronous_messages_iter};
use crate::builders::pact_builder_async::PactBuilderAsync;
use crate::builders::sync_message_builder::SyncMessageInteractionBuilder;
use crate::mock_server::http_mock_server::ValidatingHttpMockServer;
use crate::mock_server::plugin_mock_server::PluginMockServer;
use crate::PACT_CONSUMER_VERSION;
use crate::prelude::*;

use super::interaction_builder::InteractionBuilder;

/// Builder for `Pact` objects.
///
/// ```
/// use pact_consumer::prelude::*;
/// use pact_consumer::*;
///
/// let pact = PactBuilder::new("Greeting Client", "Greeting Server")
///     .interaction("asks for a greeting", "", |mut i| {
///         i.request.path("/greeting/hello");
///         i.response
///             .header("Content-Type", "application/json")
///             .json_body(json_pattern!({ "message": "hello" }));
///         i
///     })
///     .build();
///
/// // The request method and response status default as follows.
/// assert_eq!(pact.interactions()[0].as_request_response().unwrap().request.method, "GET");
/// assert_eq!(pact.interactions()[0].as_request_response().unwrap().response.status, 200);
/// ```
pub struct PactBuilder {
  pact: Box<dyn Pact + Send + Sync>,
  output_dir: Option<PathBuf>
}

impl PactBuilder {
    /// Create a new `PactBuilder`, specifying the names of the service
    /// consuming the API and the service providing it.
    pub fn new<C, P>(consumer: C, provider: P) -> Self
    where
        C: Into<String>,
        P: Into<String>,
    {
        pact_matching::matchers::configure_core_catalogue();
        pact_mock_server::configure_core_catalogue();

        let mut pact = RequestResponsePact::default();
        pact.consumer = Consumer {
            name: consumer.into(),
        };
        pact.provider = Provider {
            name: provider.into(),
        };

        if let Some(version) = PACT_CONSUMER_VERSION {
          pact.add_md_version("consumer", version);
        }

        PactBuilder { pact: pact.boxed(), output_dir: None }
    }

    /// Create a new `PactBuilder` for a V4 specification Pact, specifying the names of the service
    /// consuming the API and the service providing it.
    pub fn new_v4<C, P>(consumer: C, provider: P) -> Self
      where
        C: Into<String>,
        P: Into<String>
    {
      pact_matching::matchers::configure_core_catalogue();
      pact_mock_server::configure_core_catalogue();

      let mut pact = V4Pact {
        consumer: Consumer { name: consumer.into() },
        provider: Provider { name: provider.into() },
        .. V4Pact::default()
      };

      if let Some(version) = PACT_CONSUMER_VERSION {
        pact.add_md_version("consumer", version);
      }

      PactBuilder { pact: pact.boxed(), output_dir: None }
    }

    /// Add a plugin to be used by the test. Note this will return an async version of the Pact
    /// builder.
    ///
    /// Panics:
    /// Plugins only work with V4 specification pacts. This method will panic if the pact
    /// being built is V3 format. Use `PactBuilder::new_v4` to create a builder with a V4 format
    /// pact.
    pub async fn using_plugin(&mut self, name: &str, version: Option<String>) -> PactBuilderAsync {
      if !self.pact.is_v4() {
        panic!("Plugins require V4 specification pacts. Use PactBuilder::new_v4");
      }

      let result = load_plugin(&PluginDependency {
        name: name.to_string(),
        version,
        dependency_type: Default::default()
      }).await;

      let mut pact = self.pact.boxed();
      match result {
        Ok(plugin) => pact.add_plugin(plugin.manifest.name.as_str(), plugin.manifest.version.as_str(), None)
          .expect("Could not add plugin to pact"),
        Err(err) => panic!("Could not load plugin - {}", err)
      }

      PactBuilderAsync::from_builder(pact, self.output_dir.clone())
    }

    /// Add a new HTTP `Interaction` to the `Pact`. Needs to return a clone of the builder
    /// that is passed in.
    pub fn interaction<D, F>(&mut self, description: D, interaction_type: D, build_fn: F) -> &mut Self
    where
        D: Into<String>,
        F: FnOnce(InteractionBuilder) -> InteractionBuilder
    {
        let interaction = InteractionBuilder::new(description.into(), interaction_type.into());
        let interaction = build_fn(interaction);

        if self.pact.is_v4() {
          self.push_interaction(&interaction.build_v4())
        } else {
          self.push_interaction(&interaction.build())
        }
    }

    /// Directly add a pre-built `Interaction` to our `Pact`. Normally it's
    /// easier to use `interaction` instead of this function.
    pub fn push_interaction(&mut self, interaction: &dyn Interaction) -> &mut Self {
      trace!("Adding interaction {:?}", interaction);
      self.pact.add_interaction(interaction).unwrap();
      self
    }

  /// Return the `Pact` we've built.
  pub fn build(&self) -> Box<dyn Pact + Send + Sync> {
    trace!("Building Pact -> {:?}", self.pact);
    self.pact.boxed()
  }

  /// Sets the output directory to write pact files to
  pub fn output_dir<D: Into<PathBuf>>(&mut self, dir: D) -> &mut Self {
    self.output_dir = Some(dir.into());
    self
  }

  /// Add a new Asynchronous message `Interaction` to the `Pact`
  pub fn message_interaction<D, F>(&mut self, description: D, build_fn: F) -> &mut Self
    where
      D: Into<String>,
      F: FnOnce(MessageInteractionBuilder) -> MessageInteractionBuilder
  {
    let interaction = MessageInteractionBuilder::new(description.into());
    let interaction = build_fn(interaction);

    if let Some(plugin_data) = interaction.plugin_config() {
      let _ = self.pact.add_plugin(plugin_data.name.as_str(), plugin_data.version.as_str(),
                           Some(plugin_data.configuration.clone()));
    }

    self.push_interaction(&interaction.build())
  }


  /// Add a new synchronous message `Interaction` to the `Pact`
  pub fn synchronous_message_interaction<D, F>(&mut self, description: D, build_fn: F) -> &mut Self
    where
      D: Into<String>,
      F: FnOnce(SyncMessageInteractionBuilder) -> SyncMessageInteractionBuilder
  {
    let interaction = SyncMessageInteractionBuilder::new(description.into());
    let interaction = build_fn(interaction);

    if let Some(plugin_data) = interaction.plugin_config() {
      let _ = self.pact.add_plugin(plugin_data.name.as_str(), plugin_data.version.as_str(),
                                   Some(plugin_data.configuration.clone()));
    }

    self.push_interaction(&interaction.build())
  }

  /// Returns an iterator over the asynchronous messages in the Pact
  pub fn messages(&self) -> MessageIterator<AsynchronousMessage> {
    send_metrics(MetricEvent::ConsumerTestRun {
      interactions: self.pact.interactions().len(),
      test_framework: "pact_consumer".to_string(),
      app_name: "pact_consumer".to_string(),
      app_version: env!("CARGO_PKG_VERSION").to_string()
    });
    asynchronous_messages_iter(self.pact.as_v4_pact().unwrap())
  }

  /// Returns an iterator over the synchronous req/res messages in the Pact
  pub fn synchronous_messages(&self) -> MessageIterator<SynchronousMessage> {
    send_metrics(MetricEvent::ConsumerTestRun {
      interactions: self.pact.interactions().len(),
      test_framework: "pact_consumer".to_string(),
      app_name: "pact_consumer".to_string(),
      app_version: env!("CARGO_PKG_VERSION").to_string()
    });
    synchronous_messages_iter(self.pact.as_v4_pact().unwrap())
  }
}

impl StartMockServer for PactBuilder {
  fn start_mock_server(&self, catalog_entry: Option<&str>) -> Box<dyn ValidatingMockServer> {
    match catalog_entry {
      Some(entry_name) => match catalogue_manager::lookup_entry(entry_name) {
        Some(entry) => if entry.entry_type == CatalogueEntryType::TRANSPORT {
          PluginMockServer::start(self.build(), self.output_dir.clone(), &entry)
            .expect("Could not start the plugin mock server")
        } else {
          panic!("Catalogue entry for key '{}' is not for a network transport", entry_name);
        }
        None => panic!("Did not find a catalogue entry for key '{}'", entry_name)
      }
      None => ValidatingHttpMockServer::start(self.build(), self.output_dir.clone())
    }
  }
}

impl Drop for PactBuilder {
  fn drop(&mut self) {
    // decrement access to any plugin loaded for the Pact
    for plugin in self.pact.plugin_data() {
      let dependency = PluginDependency {
        name: plugin.name,
        version: Some(plugin.version),
        dependency_type: PluginDependencyType::Plugin
      };
      drop_plugin_access(&dependency);
    }
  }
}
