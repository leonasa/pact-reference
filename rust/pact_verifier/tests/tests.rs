use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use expectest::prelude::*;
use maplit::*;
use reqwest::Client;
use serde_json::Value;

use pact_consumer::*;
use pact_consumer::prelude::*;
use pact_models::pact::read_pact;
use pact_models::provider_states::ProviderState;
use pact_verifier::{
  FilterInfo,
  NullRequestFilterExecutor,
  PactSource,
  ProviderInfo,
  ProviderTransport,
  PublishOptions,
  VerificationOptions,
  verify_pact_internal,
  verify_provider_async
};
use pact_verifier::callback_executors::ProviderStateExecutor;

/// Get the path to one of our sample *.json files.
fn fixture_path(path: &str) -> PathBuf {
  env::current_dir()
    .expect("could not find current working directory")
    .join("tests")
    .join(path)
    .to_owned()
}

struct DummyProviderStateExecutor;

#[async_trait]
impl ProviderStateExecutor for DummyProviderStateExecutor {
  async fn call(
    self: Arc<Self>,
    _interaction_id: Option<String>,
    _provider_state: &ProviderState,
    _setup: bool,
    _client: Option<&Client>
  ) -> anyhow::Result<HashMap<String, Value>> {
    Ok(hashmap!{})
  }

  fn teardown(self: &Self) -> bool {
        return false
    }
}

#[test_log::test(tokio::test)]
async fn verify_pact_with_match_values_matcher() {
  let server = PactBuilderAsync::new("consumer", "matchValuesService")
    .interaction("request requiring matching values", "", |mut i| async move {
      i.test_name("verify_pact_with_match_values_matcher");
      i.request.method("GET");
      i.request.path("/myapp/test");
      i.response.ok().content_type("application/json").body(r#"{
        "field1": "test string",
        "field2": false,
        "field3": {
          "nested1": {
            "0": {
              "value1": "1st test value",
              "value2": 99,
              "value3": 100.0
            },
            "2": {
              "value1": "2nd test value",
              "value2": 98,
              "value3": 102.0
            }
          }
        },
        "field4": 50
      }"#);
      i
    })
    .await
    .start_mock_server(None);

  #[allow(deprecated)]
  let provider = ProviderInfo {
    name: "MatchValuesProvider".to_string(),
    host: "127.0.0.1".to_string(),
    port: server.url().port(),
    transports: vec![ ProviderTransport {
      transport: "HTTP".to_string(),
      port: server.url().port(),
      path: None,
      scheme: Some("http".to_string())
    } ],
    .. ProviderInfo::default()
  };

  let pact_file = fixture_path("match-values.json");
  let pact = read_pact(pact_file.as_path()).unwrap();
  let options: VerificationOptions<NullRequestFilterExecutor> = VerificationOptions::default();
  let provider_states = Arc::new(DummyProviderStateExecutor{});

  let result = verify_pact_internal(
    &provider,
    &FilterInfo::None,
    pact,
    &options,
    &provider_states,
    false
  ).await;

  expect!(result.unwrap().results.get(0).unwrap().result.as_ref()).to(be_ok());
}

#[test_log::test(tokio::test)]
async fn verify_pact_with_attributes_with_special_values() {
  let server = PactBuilder::new_v4("book_consumer", "book_provider")
    .interaction("create book request", "", |mut i| {
      i.test_name("verify_pact_with_attributes_with_special_values");
      i.request.method("POST");
      i.request.path("/books");
      i.request.content_type("application/json");

      i.response.ok().content_type("application/json").json_body(json_pattern!({
        "@context": "/api/contexts/Book",
        "@id": "/api/books/0114b2a8-3347-49d8-ad99-0e792c5a30e6",
        "@type": "Book",
        "title": "Voluptas et tempora repellat corporis excepturi.",
        "description": "Quaerat odit quia nisi accusantium natus voluptatem. Explicabo corporis eligendi ut ut sapiente ut qui quidem. Optio amet velit aut delectus. Sed alias asperiores perspiciatis deserunt omnis. Mollitia unde id in.",
        "author": "Melisa Kassulke",
        "%publicationDate%": "1999-02-13T00:00:00+07:00",
        "reviews": []
      }));
      i
    })
    .start_mock_server(None);

  #[allow(deprecated)]
  let provider = ProviderInfo {
    name: "BookProvider".to_string(),
    host: "127.0.0.1".to_string(),
    port: server.url().port(),
    transports: vec![ ProviderTransport {
      transport: "HTTP".to_string(),
      port: server.url().port(),
      path: None,
      scheme: Some("http".to_string())
    } ],
    .. ProviderInfo::default()
  };

  let pact_file = fixture_path("pact_with_special_chars.json");
  let pact = read_pact(pact_file.as_path()).unwrap();
  let options: VerificationOptions<NullRequestFilterExecutor> = VerificationOptions::default();
  let provider_states = Arc::new(DummyProviderStateExecutor{});

  let result = verify_pact_internal(
    &provider,
    &FilterInfo::None,
    pact,
    &options,
    &provider_states,
    false
  ).await;

  expect!(result.unwrap().results.get(0).unwrap().result.as_ref()).to(be_ok());
}

#[test_log::test(tokio::test)]
async fn verifying_a_pact_with_pending_interactions() {
  let provider = ProviderInfo {
    name: "PendingProvider".to_string(),
    host: "127.0.0.1".to_string(),
    .. ProviderInfo::default()
  };

  let pact_file = fixture_path("v4-pending-pact.json");
  let pact = read_pact(pact_file.as_path()).unwrap();
  let options: VerificationOptions<NullRequestFilterExecutor> = VerificationOptions::default();
  let provider_states = Arc::new(DummyProviderStateExecutor{});

  let result = verify_pact_internal(
    &provider,
    &FilterInfo::None,
    pact,
    &options,
    &provider_states,
    false
  ).await;

  expect!(result.as_ref().unwrap().results.get(0).unwrap().result.as_ref()).to(be_err());
  expect!(result.as_ref().unwrap().results.get(0).unwrap().pending).to(be_true());
}

#[test_log::test(tokio::test)]
async fn verifying_a_pact_with_min_type_matcher_and_child_arrays() {
  let server = PactBuilderAsync::new_v4("consumer", "Issue396Service")
    .interaction("get data request", "", |mut i| async move {
      i.test_name("verifying_a_pact_with_min_type_matcher_and_child_arrays");
      i.request.method("GET");
      i.request.path("/data");
      i.response.ok().content_type("application/json").json_body(json_pattern!({
          "parent": [
            {
              "child": [
                "a"
              ]
            },
            {
              "child": [
                "a"
              ]
            }
          ]
        }));
      i
    })
    .await
    .start_mock_server(None);

  #[allow(deprecated)]
  let provider = ProviderInfo {
    name: "Issue396Service".to_string(),
    host: "127.0.0.1".to_string(),
    port: server.url().port(),
    transports: vec![ ProviderTransport {
      transport: "HTTP".to_string(),
      port: server.url().port(),
      path: None,
      scheme: Some("http".to_string())
    } ],
    .. ProviderInfo::default()
  };

  let pact_file = fixture_path("issue396.json");
  let pact = read_pact(pact_file.as_path()).unwrap();
  let options: VerificationOptions<NullRequestFilterExecutor> = VerificationOptions::default();
  let provider_states = Arc::new(DummyProviderStateExecutor{});

  let result = verify_pact_internal(
    &provider,
    &FilterInfo::None,
    pact,
    &options,
    &provider_states,
    false
  ).await;

  expect!(result.unwrap().results.get(0).unwrap().result.as_ref()).to(be_ok());
}

#[test_log::test(tokio::test)]
async fn verify_multiple_pacts() {
  let provider = PactBuilder::new_v4("book_consumer", "book_provider")
    .interaction("a retrieve Mallory request", "", |mut i| {
      i.test_name("verify_multiple_pacts");
      i.request.method("GET");
      i.request.path("/mallory");
      i.request.query_param("name", "ron");
      i.request.query_param("status", "good");

      i.response.ok().content_type("application/json").json_body(json_pattern!({
        "result": "hello"
      }));
      i
    })
    .interaction("a retrieve test request", "", |mut i| {
      i.test_name("verify_multiple_pacts");
      i.request.method("GET");
      i.request.path("/");
      i.request.query_param("q", "p");
      i.request.query_param("q", "p2");
      i.request.query_param("r", "s");

      i.response.ok().content_type("application/json").json_body(json_pattern!({
        "responsetest": true
      }));

      i
    })
    .start_mock_server(None);

  let pact_one_file = fixture_path("pact-one.json");
  let pact_one = read_file(&pact_one_file).unwrap();
  let pact_two_file = fixture_path("pact-two.json");
  let pact_two = read_file(&pact_two_file).unwrap();

  let server = PactBuilderAsync::new("RustPactVerifier", "PactBrokerTest")
    .interaction("a request to the pact broker root", "", |mut i| async move {
      i.request
        .path("/")
        .header("Accept", "application/hal+json")
        .header("Accept", "application/json");
      i.response
        .header("Content-Type", "application/hal+json")
        .json_body(json_pattern!({
            "_links": {
                "pb:provider-pacts-for-verification": {
                  "href": like!("http://localhost/pacts/provider/{provider}/for-verification"),
                  "title": like!("Pact versions to be verified for the specified provider"),
                  "templated": like!(true)
                }
            }
        }));
      i
    })
    .await
    .interaction("a request to the pacts for verification endpoint", "", |mut i| async move {
      i.request
        .get()
        .path("/pacts/provider/Alice%20Service/for-verification")
        .header("Accept", "application/hal+json")
        .header("Accept", "application/json");
      i.response
        .header("Content-Type", "application/hal+json")
        .json_body(json_pattern!({
                "_links": {
                    "self": {
                      "href": like!("http://localhost/pacts/provider/Alice%20Service/for-verification"),
                      "title": like!("Pacts to be verified")
                    }
                }
            }));
      i
    })
    .await
    .interaction("a request for the pacts to verify", "", |mut i| async move {
      i.request
        .post()
        .path("/pacts/provider/Alice%20Service/for-verification")
        .header("Accept", "application/hal+json")
        .header("Accept", "application/json");
      i.response
        .header("Content-Type", "application/hal+json")
        .json_body(json_pattern!({
          "_embedded": {
            "pacts": [
              {
                "shortDescription": "pact-one",
                "_links": {
                  "self": {
                    "name": "pact-one",
                    "href": "/pact-one",
                    "templated": false
                  }
                },
                "verificationProperties": {
                  "pending": false,
                  "notices": []
                }
              },
              {
                "shortDescription": "pact-two",
                "_links": {
                  "self": {
                    "name": "pact-two",
                    "href": "/pact-two",
                    "templated": false
                  }
                },
                "verificationProperties": {
                  "pending": false,
                  "notices": []
                }
              }
            ]
          },
          "_links": {
              "self": {
                "href": like!("http://localhost/pacts/provider/Alice%20Service/for-verification"),
                "title": like!("Pacts to be verified")
              }
          }
      }));

      i
    })
    .await
    .interaction("pact-one", "", |mut i| async move {
      i.request
        .get()
        .path("/pact-one")
        .header("Accept", "application/hal+json")
        .header("Accept", "application/json");
      i.response
        .header("Content-Type", "application/json")
        .body(pact_one);
      i
    })
    .await
    .interaction("pact-two", "", |mut i| async move {
      i.request
        .get()
        .path("/pact-two")
        .header("Accept", "application/hal+json")
        .header("Accept", "application/json");
      i.response
        .header("Content-Type", "application/json")
        .body(pact_two);
      i
    })
    .await
    .interaction("pact-one results", "", |mut i| async move {
      i.request
        .post()
        .path("/pact-one/results")
        .header("content-type", "application/json")
        .json_body(json_pattern!({
          "providerApplicationVersion": "1.2.3",
          "success": false,
          "testResults": [
            {
              "interactionId": "pact-one",
              "mismatches": [
                {
                  "attribute": "body",
                  "description": like!("Some error message"),
                  "identifier": "$"
                },
                {
                  "attribute": "header",
                  "description": like!("Some error message"),
                  "identifier": "Content-Type"
                }
              ],
              "success": false
            }
          ],
          "verifiedBy": { "implementation": "Pact-Rust", "version": like!("1.0.0") }
        }));
      i
    })
    .await
    .interaction("pact-two results", "", |mut i| async move {
      i.request
        .post()
        .path("/pact-two/results")
        .header("Content-Type", "application/json")
        .json_body(json_pattern!({
          "providerApplicationVersion": "1.2.3",
          "success": false,
          "testResults": [
            {
              "interactionId": "pact-two",
              "mismatches": [
                {
                  "attribute": "header",
                  "description": like!("Expected header 'testreqheader' but was missing"),
                  "identifier": "testreqheader"
                }
              ],
              "success": false
            }
          ],
          "verifiedBy":{ "implementation": "Pact-Rust", "version": like!("1.0.0") }
        }));
      i
    })
    .await
    .start_mock_server(None);

  #[allow(deprecated)]
  let provider_info = ProviderInfo {
    name: "Alice Service".to_string(),
    host: "127.0.0.1".to_string(),
    port: provider.url().port(),
    transports: vec![ ProviderTransport {
      transport: "HTTP".to_string(),
      port: provider.url().port(),
      path: None,
      scheme: Some("http".to_string())
    } ],
    .. ProviderInfo::default()
  };

  let pact_source = PactSource::BrokerWithDynamicConfiguration {
    provider_name: "Alice Service".to_string(),
    broker_url: server.url().to_string(),
    enable_pending: false,
    include_wip_pacts_since: None,
    provider_tags: vec![],
    provider_branch: None,
    selectors: vec![],
    auth: None,
    links: vec![]
  };

  let verification_options: VerificationOptions<NullRequestFilterExecutor> = VerificationOptions::default();
  let provider_state_executor = Arc::new(DummyProviderStateExecutor{});
  let publish_options = PublishOptions {
    provider_version: Some("1.2.3".to_string()),
    build_url: None,
    provider_tags: vec![],
    provider_branch: None,
  };

  let result = verify_provider_async(
    provider_info,
    vec![ pact_source ],
    FilterInfo::None,
    vec![ "Consumer".to_string() ],
    &verification_options,
    Some(&publish_options),
    &provider_state_executor,
    None
  ).await;

  expect!(result.unwrap().result).to(be_false());
}

fn read_file(path: &PathBuf) -> anyhow::Result<String> {
  let mut f = File::open(path)?;
  let mut buf = String::new();
  f.read_to_string(&mut buf)?;
  Ok(buf)
}
