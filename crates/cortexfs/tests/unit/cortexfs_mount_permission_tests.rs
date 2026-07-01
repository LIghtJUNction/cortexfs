mod permission_tests {
    use cortexfs::{FuseV1Attr, FuseV1FileType};
    use fuser::{AccessFlags, Errno, OpenFlags};

    use super::super::{
        access_error, fuse_copy_file_range_error, fuse_ioctl_error, fuse_open_error,
        fuse_setattr_metadata_error, fuse_write_error,
    };

    #[test]
    fn fuse_setattr_metadata_error_maps_metadata_changes_to_readonly_filesystem() {
        assert_readonly(fuse_setattr_metadata_error(true));
    }

    #[test]
    fn fuse_setattr_metadata_error_allows_size_only_requests_to_projection() {
        assert!(fuse_setattr_metadata_error(false).is_none());
    }

    #[test]
    fn access_error_checks_socket_write_access_with_mode_bits() {
        let attr = FuseV1Attr::new(
            "agent/coder.sock".to_owned(),
            FuseV1FileType::Socket,
            0,
            0o666,
        );

        assert!(access_error(&attr, 1000, 1000, &[], AccessFlags::W_OK).is_none());
    }

    #[test]
    fn access_error_accepts_supplementary_group_bits() {
        let attr =
            FuseV1Attr::with_owner("shared".to_owned(), FuseV1FileType::Directory, 0, 0o750, 0, 42);

        assert!(access_error(&attr, 1000, 100, &[42], AccessFlags::X_OK).is_none());
    }

    #[test]
    fn access_error_allows_model_route_write_access_to_projection() {
        let attr = FuseV1Attr::with_owner(
            "model/route".to_owned(),
            FuseV1FileType::Regular,
            0,
            0o644,
            1000,
            1000,
        );

        assert!(access_error(&attr, 1000, 1000, &[], AccessFlags::W_OK).is_none());
    }

    #[test]
    fn fuse_open_error_maps_readonly_truncate_on_directory_to_is_directory() {
        let attr = FuseV1Attr::new("agent".to_owned(), FuseV1FileType::Directory, 0, 0o755);
        let flags = OpenFlags(nix::libc::O_RDONLY | nix::libc::O_TRUNC);

        assert_eq!(
            format!("{:?}", fuse_open_error(&attr, flags)),
            format!("{:?}", Some(Errno::EISDIR))
        );
    }

    #[test]
    fn fuse_open_error_maps_socket_open_to_no_device_or_address() {
        let attr = FuseV1Attr::new("agent/coder.sock".to_owned(), FuseV1FileType::Socket, 0, 0o666);

        assert_eq!(
            format!("{:?}", fuse_open_error(&attr, OpenFlags(nix::libc::O_RDONLY))),
            format!("{:?}", Some(Errno::ENXIO))
        );
    }

    #[test]
    fn fuse_open_error_allows_model_route_write_open_to_projection() {
        let attr = FuseV1Attr::new("model/route".to_owned(), FuseV1FileType::Regular, 0, 0o644);

        assert!(fuse_open_error(&attr, OpenFlags(nix::libc::O_WRONLY)).is_none());
    }

    #[test]
    fn fuse_write_error_maps_directory_writes_to_is_directory() {
        let attr = FuseV1Attr::new("agent".to_owned(), FuseV1FileType::Directory, 0, 0o755);

        assert_eq!(format!("{:?}", fuse_write_error(&attr)), format!("{:?}", Some(Errno::EISDIR)));
    }

    #[test]
    fn fuse_write_error_maps_non_control_writes_to_readonly_filesystem() {
        let attr = FuseV1Attr::new("status".to_owned(), FuseV1FileType::Regular, 0, 0o444);

        assert_readonly(fuse_write_error(&attr));
    }

    #[test]
    fn fuse_write_error_allows_writable_control_files_to_projection() {
        let attr = FuseV1Attr::new(
            "agent/worker.d/model".to_owned(),
            FuseV1FileType::Regular,
            0,
            0o644,
        );

        assert!(fuse_write_error(&attr).is_none());
    }

    #[test]
    fn fuse_write_error_allows_model_route_control_file_to_projection() {
        let attr = FuseV1Attr::new("model/route".to_owned(), FuseV1FileType::Regular, 0, 0o644);

        assert!(fuse_write_error(&attr).is_none());
    }

    #[test]
    fn fuse_copy_file_range_error_maps_to_readonly_filesystem() {
        assert_eq!(
            format!("{:?}", fuse_copy_file_range_error()),
            format!("{:?}", Errno::EROFS)
        );
    }

    #[test]
    fn fuse_ioctl_error_maps_to_inappropriate_ioctl_for_device() {
        assert_eq!(format!("{:?}", fuse_ioctl_error()), format!("{:?}", Errno::ENOTTY));
    }

    fn assert_readonly(error: Option<Errno>) {
        assert_eq!(format!("{error:?}"), format!("{:?}", Some(Errno::EROFS)));
    }
}
