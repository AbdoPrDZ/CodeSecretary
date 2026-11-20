//! `secretary-agent login | logout | status` (AG-2).
//!
//! Output goes to the terminal only. The user's own e-mail and the device
//! code are shown to them; tokens are never printed.

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use secretary_shared::api::AuthConfig;

use crate::api::{self, Warehouse};
use crate::auth::store::{KeyringStore, TokenStore, refresh_token_key};
use crate::auth::{Link, oidc};
use crate::buffer::Buffer;
use crate::cli::LoginArgs;
use crate::config::LocalConfig;
use crate::paths::AgentPaths;
use crate::state::{Account, AgentState};

pub fn login(args: LoginArgs) -> ExitCode {
    with_runtime(|paths, config| async move { run_login(paths, config, args).await })
}

pub fn logout() -> ExitCode {
    with_runtime(|paths, _config| async move { run_logout(paths).await })
}

pub fn status() -> ExitCode {
    with_runtime(|paths, config| async move { run_status(paths, config) })
}

fn with_runtime<F, Fut>(f: F) -> ExitCode
where
    F: FnOnce(AgentPaths, LocalConfig) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let result = (|| {
        let paths = AgentPaths::resolve()?;
        paths.ensure()?;
        let config = LocalConfig::load(&paths.config_file())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("could not start the async runtime")?;
        runtime.block_on(f(paths, config))
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("secretary-agent: {err:#}");
            ExitCode::from(1)
        }
    }
}

async fn run_login(paths: AgentPaths, config: LocalConfig, args: LoginArgs) -> Result<()> {
    let store: Arc<dyn TokenStore> = Arc::new(KeyringStore);
    let state_file = paths.state_file();
    let mut state = AgentState::load(&state_file)?;
    if let (Some(account), Some(id)) = (&state.account, &state.installation_id)
        && store.load(&refresh_token_key(id))?.is_some()
    {
        bail!(
            "already signed in{}; run `secretary-agent logout` first",
            account
                .email
                .as_deref()
                .map(|e| format!(" as {e}"))
                .unwrap_or_default()
        );
    }

    let server_url = args
        .server
        .unwrap_or(config.server_url)
        .trim_end_matches('/')
        .to_owned();
    let http = api::http_client()?;

    let auth: AuthConfig = http
        .get(format!("{server_url}/v1/auth/config"))
        .send()
        .await
        .with_context(|| format!("could not reach the server at {server_url}"))?
        .error_for_status()
        .context("the server did not return its sign-in settings")?
        .json()
        .await
        .context("invalid sign-in settings from the server")?;
    let discovery = oidc::discover(&http, &auth.issuer).await?;
    let code = oidc::request_device_code(&http, &discovery, &auth.client_id).await?;

    let link = code
        .verification_uri_complete
        .clone()
        .unwrap_or_else(|| code.verification_uri.clone());
    println!(
        "To sign in this device, open:\n\n    {}\n",
        code.verification_uri
    );
    println!("and enter the code:  {}\n", code.user_code);
    if !args.no_browser && webbrowser::open(&link).is_ok() {
        println!("(A browser window was opened for you.)");
    }
    println!(
        "Waiting for approval (the code expires in {} min)…",
        code.expires_in / 60
    );

    let tokens = oidc::poll_device_token(&http, &discovery, &auth.client_id, &code).await?;
    let refresh_token = tokens
        .refresh_token
        .context("the identity provider issued no refresh token (offline access not granted)")?;
    let email = oidc::unverified_claim(&tokens.access_token, "email");

    let installation_id = state.installation_id();
    store.save(&refresh_token_key(&installation_id), &refresh_token)?;
    state.account = Some(Account {
        server_url: server_url.clone(),
        issuer: auth.issuer,
        client_id: auth.client_id,
        token_endpoint: discovery.token_endpoint,
        revocation_endpoint: discovery.revocation_endpoint,
        device_id: None,
        email: email.clone(),
    });
    state.save(&state_file)?;

    let warehouse = Warehouse::new(http.clone(), Arc::new(Link::new(state_file, store, http)));
    let device_id = warehouse
        .register()
        .await
        .map_err(|err| anyhow::anyhow!("signed in, but registering this device failed: {err}"))?;

    println!(
        "\nSigned in{}. This device is registered (device {device_id}).",
        email.map(|e| format!(" as {e}")).unwrap_or_default()
    );
    println!(
        "A running agent picks this up within a minute; otherwise start it with `secretary-agent run`."
    );
    Ok(())
}

async fn run_logout(paths: AgentPaths) -> Result<()> {
    let store = KeyringStore;
    let state_file = paths.state_file();
    let mut state = AgentState::load(&state_file)?;
    let Some(account) = state.account.take() else {
        println!("Not signed in.");
        return Ok(());
    };
    if let Some(id) = &state.installation_id {
        let key = refresh_token_key(id);
        if let (Some(token), Some(endpoint)) = (store.load(&key)?, &account.revocation_endpoint) {
            let http = api::http_client()?;
            if let Err(err) = oidc::revoke(&http, endpoint, &account.client_id, &token).await {
                eprintln!(
                    "note: could not end the session at the identity provider ({err:#}); it expires on its own"
                );
            }
        }
        store.delete(&key)?;
    }
    state.save(&state_file)?;
    println!("Signed out. Buffered events stay on this device until you sign in again.");
    Ok(())
}

fn run_status(paths: AgentPaths, config: LocalConfig) -> Result<()> {
    let state = AgentState::load(&paths.state_file())?;
    match &state.account {
        Some(account) => {
            println!("signed in:  {}", account.email.as_deref().unwrap_or("yes"));
            println!("server:     {}", account.server_url);
            println!("identity:   {}", account.issuer);
            match account.device_id {
                Some(id) => println!("device:     {id}"),
                None => println!("device:     not registered yet"),
            }
        }
        None => {
            println!("signed in:  no (run `secretary-agent login`)");
            println!("server:     {}", config.server_url);
        }
    }
    if paths.buffer_file().exists() {
        let buffer = Buffer::open(&paths.buffer_file())?;
        println!("buffered:   {} event(s) waiting to upload", buffer.len()?);
    }
    Ok(())
}
