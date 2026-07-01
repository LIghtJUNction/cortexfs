macro_rules! cortexfs_mount_lifecycle {
    () => {
        fn destroy(&mut self) {
            if let Ok(mut paths) = self.paths.lock() {
                paths.clear();
            }
            if let Ok(mut counts) = self.lookup_counts.lock() {
                counts.clear();
            }
            if let Ok(mut sockets) = self.socket_overlays.lock() {
                sockets.clear();
            }
        }
    };
}
