use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum ProfileKind {
    HOST, LOCAL, GUEST
}

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    machine_id: String,
    name: String,
    vendor: String,
    kind: ProfileKind,
}

pub struct ProfileSnapshot {
    pub machine_id: String,
    pub name: String,
    pub vendor: String,
    pub kind: ProfileKind,
}

impl ProfileSnapshot {
    pub fn into_profile(self) -> Profile {
        Profile { machine_id: self.machine_id, name: self.name, vendor: self.vendor, kind: self.kind}
    }
}

impl Profile {
    pub fn get_machine_id(&self) -> &str {
        &self.machine_id
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_vendor(&self) -> &str {
        &self.vendor
    }

    pub fn get_kind(&self) -> &ProfileKind {
        &self.kind
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }
}
