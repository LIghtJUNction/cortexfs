use cortex_store::RequestId;

use crate::runtime_types::MemoryItem;
use crate::text::json_string;
use crate::{Node, NodeContent, RuntimeState};

impl RuntimeState {
    pub fn drain_memory_item_once(&mut self) -> fuse3::Result<bool> {
        let Some(request_id) = self.memory_items.keys().next().cloned() else {
            return Ok(false);
        };
        let Some(item) = self.memory_items.remove(&request_id) else {
            return Err(libc::EIO.into());
        };
        self.append_memory_item(&request_id, &item);
        self.append_audit("memory.semantic", request_id.as_str(), "drained");
        self.update_last_drained(format!("{}\n", request_id.as_str()));
        Ok(true)
    }

    fn append_memory_item(&mut self, request_id: &RequestId, item: &MemoryItem) {
        use std::fmt::Write as _;
        let Some(items_inode) = self.memory_semantic_items_inode else {
            return;
        };
        let Some(content) = self
            .nodes
            .get_mut(&items_inode)
            .and_then(|node| node.content.as_mut())
            .and_then(NodeContent::as_dynamic_mut)
        else {
            return;
        };
        let _ = writeln!(
            content,
            "{{\"id\":\"{}\",\"layer\":\"semantic\",\"content\":{},\"fingerprint\":\"{}\"}}",
            request_id.as_str(),
            json_string(&item.body),
            item.fingerprint,
        );
    }

    pub fn update_memory_search(&mut self, query: &str) {
        if let Some(inode) = self.memory_query_inode {
            self.update_dynamic_file(inode, format!("{query}\n"));
        }
        let results = self.memory_search_results(query);
        if let Some(inode) = self.memory_results_inode {
            self.update_dynamic_file(inode, results);
        }
        self.append_audit("memory.search", "query", "searched");
    }

    fn memory_search_results(&self, query: &str) -> String {
        use std::fmt::Write as _;

        if query.is_empty() {
            return String::new();
        }
        let mut results = String::new();
        if let Some(messages) = self
            .thread_messages_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
        {
            for (index, line) in messages.lines().enumerate() {
                if !line.contains(query) {
                    continue;
                }
                let _ = writeln!(
                    results,
                    "{{\"source\":\"threads/demo/messages.jsonl\",\"line\":{},\"score\":1.0,\"text\":{}}}",
                    index.saturating_add(1),
                    json_string(line),
                );
            }
        }
        if let Some(items) = self
            .memory_semantic_items_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
        {
            for (index, line) in items.lines().enumerate() {
                if !line.contains(query) {
                    continue;
                }
                let _ = writeln!(
                    results,
                    "{{\"source\":\"memory/semantic/items.jsonl\",\"line\":{},\"score\":1.0,\"text\":{}}}",
                    index.saturating_add(1),
                    json_string(line),
                );
            }
        }
        results
    }
}
