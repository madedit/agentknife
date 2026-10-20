//! The stacks AgentKnife can build with. Callers pick one by id (stored in each
//! project) and use it through [`StackProfile`] only.

use ak_types::stack::StackProfile;

mod tauri_leptos;

/// Every available stack; the first is the default for new projects.
pub fn all() -> Vec<&'static dyn StackProfile> {
    vec![&tauri_leptos::TauriLeptos]
}

/// The stack for new projects.
pub fn default_profile() -> &'static dyn StackProfile {
    &tauri_leptos::TauriLeptos
}

/// The stack with this id (from a project's `project.json`).
pub fn by_id(id: &str) -> Option<&'static dyn StackProfile> {
    all().into_iter().find(|p| p.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_stored_id() {
        let d = default_profile();
        assert_eq!(by_id(d.id()).map(|p| p.id()), Some(d.id()));
        assert!(by_id("no-such-stack").is_none());
        let ids: Vec<&str> = all().iter().map(|p| p.id()).collect();
        let mut unique = ids.clone();
        unique.dedup();
        assert_eq!(ids, unique);
    }
}
