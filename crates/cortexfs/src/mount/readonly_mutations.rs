macro_rules! cortexfs_mount_readonly_mutations {
    () => {
        fn mkdir(
            &self,
            _req: &Request,
            _parent: INodeNo,
            _name: &OsStr,
            _mode: u32,
            _umask: u32,
            reply: ReplyEntry,
        ) {
            reply.error(readonly_mutation_errno());
        }

        fn rmdir(&self, _req: &Request, _parent: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
            reply.error(readonly_mutation_errno());
        }

        fn link(
            &self,
            _req: &Request,
            _ino: INodeNo,
            _newparent: INodeNo,
            _newname: &OsStr,
            reply: ReplyEntry,
        ) {
            reply.error(readonly_mutation_errno());
        }

        fn create(
            &self,
            _req: &Request,
            _parent: INodeNo,
            _name: &OsStr,
            _mode: u32,
            _umask: u32,
            _flags: i32,
            reply: ReplyCreate,
        ) {
            reply.error(readonly_mutation_errno());
        }
    };
}
