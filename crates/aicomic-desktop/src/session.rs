use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EpisodeKey {
    pub project: i64,
    pub episode: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub text: String,
    pub title: Option<String>,
    pub dirty: bool,
    pub revision: u64,
}

#[derive(Debug, Default)]
pub struct EditorSession {
    pub current: Option<EpisodeKey>,
    pub drafts: HashMap<EpisodeKey, Draft>,
}

impl EditorSession {
    pub fn rename(&mut self, title: String) {
        if let Some(draft) = self.current.and_then(|key| self.drafts.get_mut(&key))
            && draft.title.as_ref() != Some(&title)
        {
            draft.title = Some(title);
            draft.dirty = true;
            draft.revision += 1;
        }
    }
    pub fn open(&mut self, key: EpisodeKey, saved: &str) {
        self.current = Some(key);
        self.drafts.entry(key).or_insert_with(|| Draft {
            text: saved.to_owned(),
            ..Draft::default()
        });
    }
    pub fn edit(&mut self, text: String) {
        if let Some(draft) = self.current.and_then(|key| self.drafts.get_mut(&key))
            && draft.text != text
        {
            draft.text = text;
            draft.dirty = true;
            draft.revision += 1;
        }
    }
    pub fn saved(&mut self, key: EpisodeKey, revision: u64) {
        if let Some(draft) = self.drafts.get_mut(&key)
            && draft.revision == revision
        {
            draft.dirty = false;
        }
    }
    pub fn draft(&self) -> Option<&Draft> {
        self.current.and_then(|key| self.drafts.get(&key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_edits_survive_navigation_and_need_saving() {
        let mut session = EditorSession::default();
        let key = EpisodeKey {
            project: 1,
            episode: 2,
        };
        session.open(key, "剧本");
        session.rename("更改后的标题".into());
        session.open(
            EpisodeKey {
                project: 1,
                episode: 3,
            },
            "另一集",
        );
        session.open(key, "剧本");
        assert_eq!(
            session.draft().and_then(|d| d.title.as_deref()),
            Some("更改后的标题")
        );
        assert!(session.draft().unwrap().dirty);
    }

    #[test]
    fn navigation_preserves_unsaved_chinese_script() {
        let mut session = EditorSession::default();
        let first = EpisodeKey {
            project: 1,
            episode: 2,
        };
        let second = EpisodeKey {
            project: 1,
            episode: 3,
        };
        session.open(first, "旧剧本");
        session.edit("第一集：重逢\n她推开了门。".into());
        session.open(second, "第二集");
        session.open(first, "旧剧本");
        assert_eq!(
            session.draft().map(|d| d.text.as_str()),
            Some("第一集：重逢\n她推开了门。")
        );
        assert!(session.draft().unwrap().dirty);
    }

    #[test]
    fn old_save_completion_does_not_clear_new_edits() {
        let mut session = EditorSession::default();
        let key = EpisodeKey {
            project: 1,
            episode: 2,
        };
        session.open(key, "旧");
        session.edit("第一次编辑".into());
        let revision = session.draft().unwrap().revision;
        session.edit("第二次编辑".into());
        session.saved(key, revision);
        assert!(session.draft().unwrap().dirty);
        let current_revision = session.draft().unwrap().revision;
        session.saved(key, current_revision);
        assert!(!session.draft().unwrap().dirty);
    }
}
