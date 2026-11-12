use url::Url;

#[derive(Default)]
pub struct Connection {
    pub draft_url: String,
    pub error: Option<String>,
    pending: Option<(Url, Option<u64>)>,
}

pub enum Completion {
    Connected(Url),
    Failed,
}

impl Connection {
    pub fn begin(&mut self, url: Url) {
        self.draft_url = url.to_string();
        self.error = None;
        self.pending = Some((url, None));
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    pub fn allows(&self, url: &Url, saved: &str) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(target, _)| target.origin() == url.origin())
            || Url::parse(saved).is_ok_and(|target| target.origin() == url.origin())
    }

    pub fn started(&mut self, navigation_id: u64, url: &Url, saved: &str) {
        if !matches!(url.scheme(), "http" | "https") || !self.allows(url, saved) {
            return;
        }
        if self.pending.is_none() {
            let Ok(target) = Url::parse(saved) else {
                return;
            };
            self.begin(target);
        }
        if let Some((target, id)) = &mut self.pending
            && target.origin() == url.origin()
        {
            *id = Some(navigation_id);
        }
    }

    pub fn completed(&mut self, navigation_id: u64, successful: bool) -> Option<Completion> {
        if self.pending.as_ref()?.1 != Some(navigation_id) {
            return None;
        }
        let (target, _) = self.pending.take()?;
        if successful {
            self.error = None;
            Some(Completion::Connected(target))
        } else {
            // 用户要求地址输错后仍能编辑；失败保留本次输入，只把真正成功加载的地址作为下次自动连接地址。
            self.error = Some("无法连接此服务，请检查服务地址、网络或证书后重新输入。".into());
            Some(Completion::Failed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_navigation_keeps_editable_draft_without_confirming_it() {
        let mut connection = Connection::default();
        let target = Url::parse("https://bad.example.invalid/").unwrap();
        connection.begin(target.clone());
        connection.started(1, &target, "");
        assert!(matches!(
            connection.completed(1, false),
            Some(Completion::Failed)
        ));
        assert_eq!(connection.draft_url, target.as_str());
        assert!(connection.error.is_some());
        let valid = Url::parse("https://remote.example.com/").unwrap();
        connection.begin(valid.clone());
        assert!(connection.error.is_none());
        connection.started(2, &valid, "");
        assert!(
            matches!(connection.completed(2, true), Some(Completion::Connected(url)) if url == valid)
        );
    }

    #[test]
    fn stale_or_canceled_navigation_cannot_override_a_new_connection() {
        let mut connection = Connection::default();
        let first = Url::parse("https://first.example.com/").unwrap();
        let second = Url::parse("https://second.example.com/").unwrap();
        connection.begin(first.clone());
        connection.started(1, &first, "");
        connection.begin(second.clone());
        connection.started(2, &second, "");
        assert!(connection.completed(1, false).is_none());
        assert!(connection.error.is_none());
        assert_eq!(connection.draft_url, second.as_str());
        connection.cancel();
        assert!(connection.completed(2, false).is_none());
        assert!(connection.error.is_none());
    }

    #[test]
    fn a_failed_refresh_can_return_to_the_connection_page() {
        let mut connection = Connection::default();
        let target = Url::parse("https://remote.example.com/").unwrap();
        connection.started(10, &target, target.as_str());
        assert!(matches!(
            connection.completed(10, false),
            Some(Completion::Failed)
        ));
        assert_eq!(connection.draft_url, target.as_str());
    }
}
