# rocket-session-store

rocket-session-store is a library for the rocket web framework.
It manages sessions by using cookies and a customizable store.

# Quickstart

Using this library consists of two steps:

1. Setting up the session store fairing when building the rocket.
2. Using the session request guard.

```rust
use rocket_session_store::{
	memory::MemoryStore,
	SessionStore,
	SessionResult,
	Session,
};

use rocket::{
	Rocket,
	get,
	http::private::cookie::CookieBuilder,
	routes,
	launch,
};

use std::time::Duration;

// Using the `Session` request guard

#[get("/")]
async fn index(session: Session<'_, String>) -> SessionResult<String> {
	let name: Option<String> = session.get().await?;
	if let Some(name) = name {
		Ok(format!("Hello, {}!", name))
	} else {
		Ok("Hello, world!".into())
	}
}

#[launch]
fn rocket() -> _ {
	// Instantiate a store that fits your needs and wrap it in a Box in SessionStore.
	let memory_store: MemoryStore::<String> = MemoryStore::default();
	let store: SessionStore<String> = SessionStore {
		store: Box::new(memory_store),
		name: "token".into(),
		duration: Duration::from_secs(3600 * 24 * 3),
		// The cookie builder is used to set the cookie's path and other options.
		// Name and value don't matter, they'll be overridden on each request.
		cookie_builder: CookieBuilder::new("", "")
			// Most web apps will want to use "/", but if your app is served from
			// `example.com/myapp/` for example you may want to use "/myapp/" (note the trailing
			// slash which prevents the cookie from being sent for `example.com/myapp2/`).
			.path("/")
	};

	// Attach it to a rocket by calling `fairing()`
	rocket::build().attach(store.fairing()).mount("/", routes![index])
}

```

# Security

 - When running Rocket behind a reverse proxy it is important to ensure that all `http://` requests are redirected to
   `https://` before they reach Rocket. If this is not done correctly, it is possible for a session cookie to be sent
   over an insecure connection, which would allow session hijacking (note that this is a separate issue from the `Secure`
   attribute; the `Secure` attribute is intended to prevent clients from sending cookies over insecure connections, this
   is about preventing the server from sending a cookie over an insecure connection, which is possible regardless of
   whether the `Secure` attribute is present).

 - Rocket automatically sets the `Secure` attribute on all cookies by default to prevent clients from sending cookies
   over insecure connections. When developing locally it may be necessary to disable this attribute, but it should
   always be set in production.

 - Rocket likewise defaults to setting `SameSite` to `Lax`, which, for browsers that support it, effectively prevents
   CSRF attacks **as long as** there are no GET requests that can change the application state (which is a bit vague,
   but basically if something is persisted in a database and it's not related to logging, it shouldn't be changed in a
   GET request). Current versions of all major browsers now support the `SameSite` attribute, but it's still recommended
   to use other CSRF prevention techniques as well. As of April 2022
   [Can I use](https://caniuse.com/mdn-http_headers_set-cookie_samesite_none) reports that 91.43% of users are using
   browsers that support `SameSite`.

 - To prevent session fixation attacks, it is important to regenerate the session token when a user logs in. There is an
   example of how to do this [here](Session::regenerate_token).

# Contributing

If you wish to contribute, please read [CONTRIBUTING.md](CONTRIBUTING.md).
