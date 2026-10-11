use crate::{
    apps::{self, App},
    audio::{self, Device, Route},
};
use objc2_foundation::{NSString, NSUserDefaults};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub app_name: String,
    pub device_uid: String,
    pub device_name: String,
}
#[derive(Clone, PartialEq)]
struct Signature {
    processes: Vec<u32>,
    device: Device,
    default_id: u32,
}
struct Active {
    signature: Signature,
    _route: Route,
}

#[derive(Default)]
pub struct State {
    pub devices: Vec<Device>,
    pub apps: Vec<App>,
    pub assignments: BTreeMap<String, Assignment>,
    active: HashMap<String, Active>,
    failed: HashMap<String, Signature>,
    pub errors: HashMap<String, String>,
    pub discovery_error: Option<String>,
    pub default_id: u32,
    pub paused: bool,
}

impl State {
    pub fn load() -> Self {
        let mut state = Self::default();
        if let Some(json) = NSUserDefaults::standardUserDefaults()
            .stringForKey(&NSString::from_str("assignments.v1"))
        {
            match serde_json::from_str(&json.to_string()) {
                Ok(assignments) => state.assignments = assignments,
                Err(e) => {
                    state.discovery_error = Some(format!("Could not load saved assignments: {e}"))
                }
            }
        }
        state
    }
    pub fn save(&self) {
        let json = serde_json::to_string(&self.assignments).expect("string map is serializable");
        unsafe {
            NSUserDefaults::standardUserDefaults().setObject_forKey(
                Some(&NSString::from_str(&json)),
                &NSString::from_str("assignments.v1"),
            );
        }
    }
    pub fn refresh(&mut self) {
        let result = audio::devices()
            .and_then(|devices| Ok((devices, apps::discover()?, audio::default_output()?)));
        match result {
            Ok((devices, apps, default_id)) => {
                self.devices = devices;
                self.apps = apps;
                self.default_id = default_id;
                self.discovery_error = None;
            }
            Err(error) => {
                self.discovery_error = Some(error);
                self.active.clear();
                return;
            }
        }
        self.reconcile();
    }
    fn reconcile(&mut self) {
        let desired: HashMap<_, _> = if self.paused {
            HashMap::new()
        } else {
            self.assignments
                .iter()
                .filter_map(|(key, assignment)| {
                    let app = self.apps.iter().find(|a| &a.key == key)?;
                    let device = self
                        .devices
                        .iter()
                        .find(|d| d.uid == assignment.device_uid)?;
                    Some((
                        key.clone(),
                        Signature {
                            processes: app.processes.clone(),
                            device: device.clone(),
                            default_id: self.default_id,
                        },
                    ))
                })
                .collect()
        };
        self.active
            .retain(|key, active| desired.get(key) == Some(&active.signature));
        self.failed
            .retain(|key, signature| desired.get(key) == Some(signature));
        self.errors.retain(|key, _| self.failed.contains_key(key));
        for (key, signature) in desired {
            if self.active.contains_key(&key) || self.failed.get(&key) == Some(&signature) {
                continue;
            }
            match Route::start(&signature.processes, &signature.device) {
                Ok(route) => {
                    self.active.insert(
                        key,
                        Active {
                            signature,
                            _route: route,
                        },
                    );
                }
                Err(error) => {
                    self.errors.insert(key.clone(), error);
                    self.failed.insert(key, signature);
                }
            }
        }
    }
    pub fn assign(&mut self, key: String, assignment: Option<Assignment>) {
        self.active.remove(&key);
        self.failed.remove(&key);
        self.errors.remove(&key);
        if let Some(value) = assignment {
            self.assignments.insert(key, value);
        } else {
            self.assignments.remove(&key);
        }
        self.save();
        self.refresh();
    }
    pub fn retry(&mut self) {
        self.failed.clear();
        self.errors.clear();
        self.refresh();
    }
    pub fn stop(&mut self) {
        self.active.clear();
    }
}
