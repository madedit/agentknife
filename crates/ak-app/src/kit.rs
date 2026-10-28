//! AgentKnife's kit in a project (`ak/`, `Template::kit`): when the project's
//! copy differs from this AgentKnife's (an older version, or edits made before
//! the AI's tools refused them), the window offers to replace it whole. A
//! version is saved before and after; the AI hears of it with the next
//! message (and over MCP in `status`).

use ak_project::{CheckpointKind, Timeline};
use ak_types::i18n::I18n;
use ak_types::ipc::{KitChanges, PROJECT_CHANGED_EVENT};
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::state::AppState;
use crate::{Res, fail, ui_locale};

/// What the AI is told after an update.
pub fn note(files: &[String], pack_version: &str) -> Option<String> {
    (!files.is_empty()).then(|| {
        format!(
            "The user updated AgentKnife's kit in `ak/` (ak-bridge, ak-ui) to the version of \
             knowledge pack {pack_version} ({} files changed). If the app no longer builds \
             where it used something the old kit had, or an earlier edit inside `ak/`, change \
             the app's code, never `ak/`; topic `ak-ui` describes what is there now.",
            files.len()
        )
    })
}

/// What the AI hears while the kit is older (MCP `status`).
pub fn outdated_note(files: &[String]) -> Option<String> {
    (!files.is_empty()).then(|| {
        format!(
            "AgentKnife's kit in `ak/` differs from this AgentKnife's ({} files): the user can \
             update it in AgentKnife's window. Do not change `ak/` yourself; some ak-ui \
             features in topic `ak-ui` may be missing until then.",
            files.len()
        )
    })
}

/// A version's name in the user's language.
fn label(id: &str) -> String {
    I18n::new(ui_locale())
        .map(|i| i.t(id))
        .unwrap_or_else(|_| id.to_owned())
}

#[tauri::command]
pub async fn update_kit<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Res<KitChanges> {
    let (root, preview, working, project) = {
        let inner = state.inner.lock().await;
        let open = inner
            .open
            .as_ref()
            .ok_or_else(|| fail("no project is open"))?;
        (
            open.root.clone(),
            open.preview.clone(),
            open.cancel.is_some(),
            open.project.clone(),
        )
    };
    if working || crate::mcp::busy(&state) {
        return Err(fail(
            "the AI is changing the project; update after it finishes",
        ));
    }
    let (before, after) = (
        label("kit-before-update-version"),
        label("kit-updated-version"),
    );
    let at = root.clone();
    let (files, version) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let project = project.lock().map_err(|e| e.to_string())?;
        let profile = crate::home::profile_of(&project);
        // Work not saved in a version yet is saved first, so it is kept.
        Timeline::open(&at)
            .and_then(|mut t| t.checkpoint(CheckpointKind::Turn, &before, None))
            .map_err(|e| e.to_string())?;
        let files = project.update_kit(profile).map_err(|e| e.to_string())?;
        if !files.is_empty() {
            Timeline::open(&at)
                .and_then(|mut t| t.checkpoint(CheckpointKind::Task, &after, None))
                .map_err(|e| e.to_string())?;
        }
        Ok((files, profile.knowledge_pack().version))
    })
    .await
    .map_err(fail)?
    .map_err(fail)?;
    {
        let mut inner = state.inner.lock().await;
        if let Some(open) = inner.open.as_mut().filter(|o| o.root == root) {
            open.kit_outdated.clear();
            open.kit_note = note(&files, version);
            open.unannounced_kit.clone_from(&open.kit_note);
        }
    }
    let _ = preview.files_changed(&files).await;
    let _ = app.emit(PROJECT_CHANGED_EVENT, ());
    Ok(KitChanges { files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ai_hears_what_changed_and_where_not_to_fix_it() {
        assert_eq!(note(&[], "0.2.27"), None);
        assert_eq!(outdated_note(&[]), None);
        let files = ["ak/ak-ui/src/button.rs".to_owned()];
        let n = note(&files, "0.2.27").unwrap();
        assert!(n.contains("0.2.27") && n.contains("1 files") && n.contains("never `ak/`"));
        assert!(
            outdated_note(&files)
                .unwrap()
                .contains("Do not change `ak/`")
        );
    }
}
