macro_rules! cortexfs_mount_readonly_mutations {
    () => {
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

    };
}
