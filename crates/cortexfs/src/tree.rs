use crate::ROOT_INODE;
use fuse3::{FileType, Inode, Timestamp};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct StaticTree {
    pub nodes: BTreeMap<Inode, Node>,
    pub paths: BTreeMap<Vec<String>, Inode>,
}

impl StaticTree {
    pub fn path_inode(&self, components: &[&str]) -> Option<Inode> {
        let path = components
            .iter()
            .map(|component| (*component).to_owned())
            .collect::<Vec<_>>();
        self.paths.get(&path).copied()
    }

    pub fn path_inode_owned(&self, components: &[String]) -> Option<Inode> {
        self.paths.get(components).copied()
    }

    pub fn inode_path(&self, inode: Inode) -> Option<&[String]> {
        self.paths
            .iter()
            .find_map(|(path, path_inode)| (*path_inode == inode).then_some(path.as_slice()))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Node {
    pub inode: Inode,
    pub name: String,
    pub kind: FileType,
    pub content: Option<NodeContent>,
    pub children: Vec<Inode>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NodeContent {
    Static(&'static str),
    Owned(String),
    Dynamic(String),
}

impl NodeContent {
    pub fn as_str(&self) -> &str {
        match *self {
            Self::Static(content) => content,
            Self::Owned(ref content) | Self::Dynamic(ref content) => content.as_str(),
        }
    }

    pub fn as_dynamic_mut(&mut self) -> Option<&mut String> {
        match *self {
            Self::Static(_) | Self::Owned(_) => None,
            Self::Dynamic(ref mut content) => Some(content),
        }
    }
}

impl Node {
    pub fn dir(inode: Inode, name: impl Into<String>) -> Self {
        Self {
            inode,
            name: name.into(),
            kind: FileType::Directory,
            content: None,
            children: Vec::new(),
        }
    }

    pub fn file(inode: Inode, name: impl Into<String>, content: &'static str) -> Self {
        Self {
            inode,
            name: name.into(),
            kind: FileType::RegularFile,
            content: Some(NodeContent::Static(content)),
            children: Vec::new(),
        }
    }

    pub fn owned_file(inode: Inode, name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            inode,
            name: name.into(),
            kind: FileType::RegularFile,
            content: Some(NodeContent::Owned(content.into())),
            children: Vec::new(),
        }
    }

    pub fn dynamic_file(inode: Inode, name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            inode,
            name: name.into(),
            kind: FileType::RegularFile,
            content: Some(NodeContent::Dynamic(content.into())),
            children: Vec::new(),
        }
    }

    pub fn socket(inode: Inode, name: impl Into<String>) -> Self {
        Self {
            inode,
            name: name.into(),
            kind: FileType::Socket,
            content: None,
            children: Vec::new(),
        }
    }

    pub const fn inode(&self) -> Inode {
        self.inode
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn kind(&self) -> FileType {
        self.kind
    }

    pub const fn is_dir(&self) -> bool {
        matches!(self.kind, FileType::Directory)
    }

    pub fn content(&self) -> Option<&str> {
        self.content.as_ref().map(NodeContent::as_str)
    }

    pub fn children(&self) -> &[Inode] {
        &self.children
    }

    pub fn attr(&self, uid: u32, gid: u32) -> fuse3::raw::prelude::FileAttr {
        self.attr_for_mount(uid, gid, false)
    }

    pub fn attr_for_mount(
        &self,
        uid: u32,
        gid: u32,
        multi_user: bool,
    ) -> fuse3::raw::prelude::FileAttr {
        let size = self
            .content()
            .map(str::len)
            .map(u64::try_from)
            .transpose()
            .unwrap_or_default()
            .unwrap_or(0);
        self.attr_with_size(uid, gid, size, multi_user)
    }

    pub fn attr_with_size(
        &self,
        uid: u32,
        gid: u32,
        size: u64,
        multi_user: bool,
    ) -> fuse3::raw::prelude::FileAttr {
        let perm = if self.is_writable_submit_dir() {
            if multi_user { 0o777 } else { 0o755 }
        } else if self.is_socket() {
            0o666
        } else if self.is_dir() {
            0o555
        } else if self.is_dynamic_file() {
            if multi_user { 0o666 } else { 0o644 }
        } else {
            0o444
        };
        fuse3::raw::prelude::FileAttr {
            ino: self.inode,
            size,
            blocks: 1,
            atime: Timestamp::new(0, 0),
            mtime: Timestamp::new(0, 0),
            ctime: Timestamp::new(0, 0),
            kind: self.kind,
            perm,
            nlink: if self.is_dir() { 2 } else { 1 },
            uid,
            gid,
            rdev: 0,
            blksize: 512,
        }
    }

    pub fn is_dynamic_file(&self) -> bool {
        matches!(self.content, Some(NodeContent::Dynamic(_)))
    }

    pub const fn is_socket(&self) -> bool {
        matches!(self.kind, FileType::Socket)
    }

    fn is_writable_submit_dir(&self) -> bool {
        self.is_dir() && matches!(self.name.as_str(), "inbox" | "pending" | "claim" | "lease")
    }
}

pub fn build_path_index(nodes: &BTreeMap<Inode, Node>) -> BTreeMap<Vec<String>, Inode> {
    let mut paths = BTreeMap::new();
    index_children(nodes, ROOT_INODE, &mut Vec::new(), &mut paths);
    paths
}

fn index_children(
    nodes: &BTreeMap<Inode, Node>,
    parent: Inode,
    prefix: &mut Vec<String>,
    paths: &mut BTreeMap<Vec<String>, Inode>,
) {
    let Some(parent_node) = nodes.get(&parent) else {
        return;
    };
    for child in parent_node.children() {
        let Some(node) = nodes.get(child) else {
            continue;
        };
        prefix.push(node.name().to_owned());
        paths.insert(prefix.clone(), node.inode());
        if node.is_dir() {
            index_children(nodes, node.inode(), prefix, paths);
        }
        let _ = prefix.pop();
    }
}
