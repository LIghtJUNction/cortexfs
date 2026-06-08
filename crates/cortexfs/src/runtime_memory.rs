use cortex_store::RequestId;

use crate::runtime_state::MemoryLayerInodes;
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
        self.append_audit(
            format!("memory.{}", item.layer).as_str(),
            request_id.as_str(),
            "drained",
        );
        self.update_last_drained(format!("{}\n", request_id.as_str()));
        Ok(true)
    }

    fn append_memory_item(&mut self, request_id: &RequestId, item: &MemoryItem) {
        self.append_memory_layer_item(
            item.layer,
            request_id.as_str(),
            &item.body,
            &item.fingerprint,
        );
    }

    pub fn append_memory_layer_item(
        &mut self,
        layer: &str,
        id: &str,
        content_value: &str,
        fingerprint: &str,
    ) {
        use std::fmt::Write as _;
        let Some(items_inode) = self.memory_layer_items.inode_for(layer) else {
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
            "{{\"id\":\"{}\",\"layer\":\"{}\",\"content\":{},\"fingerprint\":\"{}\"}}",
            id,
            layer,
            json_string(content_value),
            fingerprint,
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
                    "{{\"source\":\"home/1000/thread/demo/messages.jsonl\",\"line\":{},\"score\":1.0,\"text\":{}}}",
                    index.saturating_add(1),
                    json_string(line),
                );
            }
        }
        for (layer, inode) in self.memory_layer_items.entries() {
            append_memory_layer_search_results(&mut results, layer, inode, query, |inode| {
                self.nodes.get(&inode).and_then(Node::content)
            });
        }
        results
    }
}

impl MemoryLayerInodes {
    fn entries(self) -> [(&'static str, Option<fuse3::Inode>); 5] {
        [
            ("working", self.working),
            ("episodic", self.episodic),
            ("semantic", self.semantic),
            ("procedural", self.procedural),
            ("profile", self.profile),
        ]
    }

    fn inode_for(self, layer: &str) -> Option<fuse3::Inode> {
        match layer {
            "working" => self.working,
            "episodic" => self.episodic,
            "semantic" => self.semantic,
            "procedural" => self.procedural,
            "profile" => self.profile,
            _ => None,
        }
    }
}

fn append_memory_layer_search_results<'a>(
    results: &mut String,
    layer: &str,
    inode: Option<fuse3::Inode>,
    query: &str,
    content_for_inode: impl Fn(fuse3::Inode) -> Option<&'a str>,
) {
    use std::fmt::Write as _;

    let Some(items) = inode.and_then(content_for_inode) else {
        return;
    };
    for (index, line) in items.lines().enumerate() {
        if !line.contains(query) {
            continue;
        }
        let _ = writeln!(
            results,
            "{{\"source\":\"home/1000/memory/{layer}/items.jsonl\",\"line\":{},\"score\":1.0,\"text\":{}}}",
            index.saturating_add(1),
            json_string(line),
        );
    }
}
