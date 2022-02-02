use std::{
	marker::PhantomData,
	time::Duration,
};

use redis::{
	Client,
	ConnectionLike,
};
use rocket::serde::DeserializeOwned;
use serde::Serialize;
use serde_json::{
	from_slice,
	to_string,
};

use crate::{
	SessionError,
	SessionResult,
	Store,
};

pub struct RedisStore<T> {
	client: Client,
	_marker: PhantomData<T>,
}

impl<T> RedisStore<T> {
	pub fn new(client: Client) -> Self {
		Self {
			client,
			_marker: PhantomData::default(),
		}
	}
}

#[rocket::async_trait]
impl<T> Store for RedisStore<T>
where
	T: Serialize + DeserializeOwned + Send + Sync,
{
	type Value = T;

	async fn get(&self, id: &str) -> SessionResult<Option<T>> {
		let mut cmd = redis::cmd("GET");
		cmd.arg(id);
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		let val = con.req_command(&cmd).map_err(|_| SessionError)?;
		use redis::Value::*;
		Ok(match val {
			Nil => None,
			Data(ref bytes) => Some(from_slice(bytes).expect("Failed to deserialize")),
			_ => None,
		})
	}

	async fn set(&self, id: &str, value: Self::Value, duration: Duration) -> SessionResult<()> {
		let mut cmd = redis::cmd("SET");
		cmd.arg(id);
		let serialized = to_string(&value).expect("Failed to serialize");
		cmd.arg(serialized);
		cmd.arg("EX");
		cmd.arg(duration.as_secs());
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		con.req_command(&cmd).map_err(|_| SessionError)?;

		Ok(())
	}

	async fn touch(&self, id: &str, duration: Duration) -> SessionResult<()> {
		let mut cmd = redis::cmd("EXPIRE");
		cmd.arg(id);
		cmd.arg(duration.as_secs());
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		con.req_command(&cmd).map_err(|_| SessionError)?;

		Ok(())
	}

	async fn remove(&self, id: &str) -> SessionResult<()> {
		let mut cmd = redis::cmd("DEL");
		cmd.arg(id);
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		con.req_command(&cmd).map_err(|_| SessionError)?;

		Ok(())
	}
}
