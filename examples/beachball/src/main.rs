use dropshot::endpoint;
use dropshot::ApiDescription;
use dropshot::ConfigDropshot;
use dropshot::ConfigLogging;
use dropshot::ConfigLoggingLevel;
use dropshot::HttpError;
use dropshot::HttpResponseOk;
use dropshot::HttpServerStarter;
use dropshot::RequestContext;
use schemars::JsonSchema;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize, JsonSchema)]
struct Spaceship {
    ship: String,
    color: String,
    ami_id: String,
}

async fn get_ami_id() -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();

    // Get the IMDSv2 session token
    let token_resp = client
        .put("http://169.254.169.254/latest/api/token")
        .header("X-aws-ec2-metadata-token-ttl-seconds", "21600")
        .send()
        .await?;

    let token = token_resp.text().await?;

    let resp = client
        .get("http://169.254.169.254/latest/meta-data/ami-id")
        .header("X-aws-ec2-metadata-token", token)
        .send()
        .await?;

    resp.text().await
}

#[endpoint {
    method = GET,
    path = "/",
}]
async fn tengu_index(
    _rqctx: RequestContext<Arc<()>>,
) -> Result<HttpResponseOk<Spaceship>, HttpError> {
    let ami_id = get_ami_id()
        .await
        .map_err(|err| HttpError::for_internal_error(err.to_string()))?;

    let tengu = Spaceship {
        ship: String::from("yes"),
        color: String::from("grey"),
        ami_id,
    };
    Ok(HttpResponseOk(tengu))
}

#[tokio::main]
async fn main() -> Result<(), String> {
    // Set up a logger.
    let log = ConfigLogging::StderrTerminal {
        level: ConfigLoggingLevel::Info,
    }
    .to_logger("minimal-example")
    .map_err(|e| e.to_string())?;

    // Describe the API.
    let mut api = ApiDescription::new();
    api.register(tengu_index).unwrap();
    // Register API functions -- see detailed example or ApiDescription docs.

    // Start the server.
    let server = HttpServerStarter::new(
        &ConfigDropshot {
            bind_address: "127.0.0.1:8000".parse().unwrap(),
            request_body_max_bytes: 1024,
            tls: None,
        },
        api,
        Arc::new(()),
        &log,
    )
    .map_err(|error| format!("failed to start server: {}", error))?
    .start();

    server.await
}
