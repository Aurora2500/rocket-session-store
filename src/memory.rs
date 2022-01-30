//!	An in-memory implementationof a session store.
//! 
//! This module provides [MemoryStore], an implementation of [Store]
//! to be used for testing and development. It is not optimized for production
//! and thus you should use another store to use it in the real world.

use std::{
	collections::HashMap,
	time::{
		Duration,
		Instant,
	},
};

use rocket::tokio::sync::{
	Mutex,
	RwLock,
};

use crate::Store;

/// An in memory implementation of a session store using hashmaps.
/// Do note that this implementation is just for testing purposes,
/// and should not be used in any real world application.
pub struct MemoryStore<T> {
	map: RwLock<HashMap<String, Mutex<MemoryStoreFrame<T>>>>,
}

struct MemoryStoreFrame<T> {
	value: T,
	expiry: Instant,
}

impl<T> Default for MemoryStore<T> {
	fn default() -> Self {
		Self::new()
	}
}

impl<T> MemoryStore<T> {
	pub fn new() -> Self {
		Self {
			map: RwLock::default(),
		}
	}
}

#[rocket::async_trait]
impl<T> Store for MemoryStore<T>
where
	T: Send + Sync + Clone,
{
	type Value = T;

	async fn get(&self, id: &str) -> Option<Self::Value> {
		let lock = self.map.read().await;
		if let Some(frame) = lock.get(id) {
			let frame_lock = frame.lock().await;
			if frame_lock.expiry.checked_duration_since(Instant::now()).is_some() {
				return Some(frame_lock.value.clone());
			};
		};
		None
	}

	async fn set(&self, id: &str, value: Self::Value, expiry: Duration) {
		let mut lock = self.map.write().await;
		let frame = MemoryStoreFrame {
			value,
			expiry: Instant::now() + expiry,
		};
		lock.insert(id.into(), Mutex::new(frame));
	}

	async fn touch(&self, id: &str, duration: Duration) {
		let lock = self.map.read().await;
		if let Some(frame) = lock.get(id) {
			let mut frame_lock = frame.lock().await;
			frame_lock.expiry = Instant::now() + duration;
		};
	}

	async fn remove(&self, id: &str) {
		let mut lock = self.map.write().await;
		lock.remove(id);
	}
}
