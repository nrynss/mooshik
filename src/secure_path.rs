//! Race-resistant private filesystem operations.
//!
//! Unix uses directory descriptors and *at syscalls so an attacker cannot
//! redirect a later path lookup by swapping a parent component. Other
//! platforms fail closed because the equivalent guarantee is not available in
//! this small portability layer yet.

use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    path::{Component, Path},
};

#[cfg(unix)]
use rand::{rngs::OsRng, TryRngCore};

#[cfg(unix)]
use std::{
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
};

#[cfg(unix)]
fn invalid_path() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "unsafe path")
}

#[cfg(unix)]
fn component_name(component: Component<'_>) -> io::Result<&OsStr> {
    match component {
        Component::Normal(name) => Ok(name),
        Component::CurDir | Component::RootDir => Err(invalid_path()),
        Component::ParentDir | Component::Prefix(_) => Err(invalid_path()),
    }
}

#[cfg(unix)]
fn as_c_string(name: &OsStr) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(name.as_bytes()).map_err(|_| invalid_path())
}

#[cfg(unix)]
fn file_from_fd(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: this function takes ownership of a newly returned fd.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

#[cfg(unix)]
fn open_directory_at(parent: libc::c_int, name: &OsStr) -> io::Result<File> {
    let name = as_c_string(name)?;
    // SAFETY: name is a valid NUL-free C string and parent is an open fd.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    file_from_fd(fd)
}

#[cfg(unix)]
fn mkdir_at(parent: libc::c_int, name: &OsStr, mode: u32) -> io::Result<()> {
    let name = as_c_string(name)?;
    // SAFETY: name is a valid NUL-free C string and parent is an open fd.
    let result = unsafe { libc::mkdirat(parent, name.as_ptr(), mode as libc::mode_t) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn creation_parent_is_private(parent: libc::c_int) -> io::Result<()> {
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: parent is an open descriptor and metadata is valid storage.
    if unsafe { libc::fstat(parent, metadata.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat initialized metadata on success.
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "creation parent is not a directory",
        ));
    }
    let mode = metadata.st_mode & 0o7777;
    let sticky = mode & 0o1000 != 0;
    let writable_by_other = mode & 0o022 != 0;
    // A non-private parent lets an unrelated uid rename the staging entry.
    // A sticky directory (for example /tmp) is safe for entries owned by the
    // current uid, so it is allowed as the outer temporary parent.
    if writable_by_other && !sticky {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "creation parent is writable by another user",
        ));
    }
    if !sticky && metadata.st_uid != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "creation parent is not owned by the current user",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn directory_entry_identity(
    parent: libc::c_int,
    name: &OsStr,
) -> io::Result<(libc::dev_t, libc::ino_t)> {
    let name = as_c_string(name)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: name is NUL-free, parent is an open directory descriptor, and
    // metadata is valid storage. The snapshot is taken before openat, so a
    // replacement after mkdir cannot make the later descriptor look like it.
    if unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstatat initialized metadata on success.
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "staging entry is not a directory",
        ));
    }
    Ok((metadata.st_dev, metadata.st_ino))
}

#[cfg(unix)]
fn descriptor_matches_identity(
    directory: &File,
    identity: (libc::dev_t, libc::ino_t),
) -> io::Result<bool> {
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: directory owns a valid descriptor and metadata is valid storage.
    if unsafe { libc::fstat(directory.as_raw_fd(), metadata.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat initialized metadata on success.
    let metadata = unsafe { metadata.assume_init() };
    Ok((metadata.st_dev, metadata.st_ino) == identity)
}

#[cfg(target_os = "linux")]
fn install_directory_noreplace(
    parent: libc::c_int,
    temporary: &OsStr,
    name: &OsStr,
) -> io::Result<()> {
    let temporary = as_c_string(temporary)?;
    let name = as_c_string(name)?;
    // renameat2(RENAME_NOREPLACE) atomically moves the staging pathname
    // without replacing the requested leaf. It is path-based; the caller
    // verifies the destination identity before returning the retained fd.
    // Never fall back to renameat.
    // SAFETY: both names are NUL-free and parent is an open directory fd.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent,
            temporary.as_ptr(),
            parent,
            name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn install_directory_noreplace(
    parent: libc::c_int,
    temporary: &OsStr,
    name: &OsStr,
) -> io::Result<()> {
    let temporary = as_c_string(temporary)?;
    let name = as_c_string(name)?;
    // macOS exposes the equivalent path-based no-replace operation as
    // renameatx_np. Filesystems that do not implement RENAME_EXCL fail
    // closed; the caller verifies the destination identity before returning
    // the retained fd.
    // SAFETY: both names are NUL-free and parent is an open directory fd.
    let result = unsafe {
        libc::renameatx_np(
            parent,
            temporary.as_ptr(),
            parent,
            name.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn install_directory_noreplace(_: libc::c_int, _: &OsStr, _: &OsStr) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic directory installation is unavailable on this platform",
    ))
}

#[cfg(unix)]
fn temporary_directory_name() -> io::Result<OsString> {
    let mut random = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| io::Error::other("secure random generation failed"))?;
    let mut name = OsString::from(".mooshik-stage-");
    for byte in random {
        use std::fmt::Write;
        write!(&mut name, "{byte:02x}").expect("writing to OsString cannot fail");
    }
    Ok(name)
}

/// Create a directory and retain its descriptor without accepting a pathname
/// replacement in the creation/open window. A private, unpredictable staging
/// directory is opened first, then atomically installed with a no-replace
/// kernel operation. Unsupported filesystems and competing targets fail closed.
#[cfg(unix)]
fn create_or_open_directory_at(
    parent: libc::c_int,
    name: &OsStr,
    mode: u32,
) -> io::Result<(File, bool)> {
    create_or_open_directory_at_with_hooks(parent, name, mode, |_| Ok(()), |_| Ok(()))
}

#[cfg(unix)]
fn create_or_open_directory_at_with_hooks<F, G>(
    parent: libc::c_int,
    name: &OsStr,
    mode: u32,
    after_mkdir: F,
    before_install: G,
) -> io::Result<(File, bool)>
where
    F: FnOnce(&OsStr) -> io::Result<()>,
    G: FnOnce(&OsStr) -> io::Result<()>,
{
    match open_directory_at(parent, name) {
        Ok(directory) => return Ok((directory, false)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    creation_parent_is_private(parent)?;
    let temporary = temporary_directory_name()?;
    mkdir_at(parent, &temporary, mode)?;
    let identity = match directory_entry_identity(parent, &temporary) {
        Ok(identity) => identity,
        Err(error) => {
            preserve_staging_directory(parent, &temporary, None);
            return Err(error);
        }
    };
    if let Err(error) = after_mkdir(&temporary) {
        preserve_staging_directory(parent, &temporary, Some(identity));
        return Err(error);
    }
    let directory = match open_directory_at(parent, &temporary) {
        Ok(directory) => directory,
        Err(error) => {
            preserve_staging_directory(parent, &temporary, Some(identity));
            return Err(error);
        }
    };
    match descriptor_matches_identity(&directory, identity) {
        Ok(true) => {}
        Ok(false) => {
            // The pathname may now belong to an attacker. Cleanup below is
            // descriptor-checked and therefore leaves a changed pathname alone.
            preserve_staging_directory(parent, &temporary, Some(identity));
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "staging directory changed during creation",
            ));
        }
        Err(error) => {
            preserve_staging_directory(parent, &temporary, Some(identity));
            return Err(error);
        }
    }
    if let Err(error) = chmod_fd(&directory, mode) {
        preserve_staging_directory(parent, &temporary, Some(identity));
        return Err(error);
    }
    if let Err(error) = before_install(&temporary) {
        preserve_staging_directory(parent, &temporary, Some(identity));
        return Err(error);
    }
    match install_directory_noreplace(parent, &temporary, name) {
        Ok(()) => {
            // renameat2/renameatx_np is necessarily pathname-based for a
            // directory source. Re-open the installed entry and compare its
            // identity before returning the retained descriptor. If the
            // source pathname was replaced after our fstat, fail closed and
            // never let callers initialize an unbound directory.
            let installed = open_directory_at(parent, name).and_then(|installed| {
                if descriptor_matches_identity(&installed, identity)? {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "installed directory identity changed",
                    ))
                }
            });
            match installed {
                Ok(()) => Ok((directory, true)),
                Err(error) => Err(error),
            }
        }
        Err(error) => {
            preserve_staging_directory(parent, &temporary, Some(identity));
            Err(error)
        }
    }
}

#[cfg(unix)]
fn chmod_fd(file: &File, mode: u32) -> io::Result<()> {
    // SAFETY: file owns a valid fd and mode is a normal Unix permission mask.
    let result = unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
pub(crate) fn open_dir(path: &Path, create: bool, mode: u32) -> io::Result<File> {
    open_dir_with_status(path, create, mode).map(|(file, _)| file)
}

/// Open a directory without following path symlinks. The boolean reports
/// whether this call had to create any path component. Callers use it to
/// distinguish a newly-created application home from an arbitrary existing
/// directory before making any changes to that directory.
#[cfg(unix)]
pub(crate) fn open_dir_with_status(
    path: &Path,
    create: bool,
    mode: u32,
) -> io::Result<(File, bool)> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(invalid_path());
    }
    let components: Vec<&OsStr> = path
        .components()
        .filter_map(|component| match component {
            Component::RootDir | Component::CurDir => None,
            other => Some(component_name(other)),
        })
        .collect::<io::Result<Vec<_>>>()?;
    let component_count = components.len();
    let mut created = false;
    let mut current = if path.is_absolute() {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        options.open("/")?
    } else {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        options.open(".")?
    };
    for (index, name) in components.into_iter().enumerate() {
        match open_directory_at(current.as_raw_fd(), name) {
            Ok(next) => current = next,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                let (next, made) = create_or_open_directory_at(current.as_raw_fd(), name, mode)?;
                created = index + 1 == component_count && made;
                current = next;
            }
            Err(error) => return Err(error),
        }
    }
    Ok((current, created))
}

#[cfg(unix)]
pub(crate) fn set_dir_mode(directory: &File, mode: u32) -> io::Result<()> {
    chmod_fd(directory, mode)
}

/// Check whether a directory descriptor contains any entries other than `.`
/// and `..`.  The descriptor is duplicated before `fdopendir` takes ownership,
/// so the caller's descriptor remains valid for subsequent *at operations.
#[cfg(unix)]
pub(crate) fn is_empty_dir(directory: &File) -> io::Result<bool> {
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: duplicate is a valid descriptor owned by this function. On
    // failure fdopendir does not take ownership, so close it explicitly.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe { libc::close(duplicate) };
        return Err(io::Error::last_os_error());
    }
    let mut empty = true;
    loop {
        // SAFETY: stream is a valid directory stream until closed below.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            break;
        }
        // SAFETY: d_name is NUL-terminated by readdir.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() != b"." && name.to_bytes() != b".." {
            empty = false;
            break;
        }
    }
    // SAFETY: stream is valid and owns duplicate.
    unsafe { libc::closedir(stream) };
    Ok(empty)
}

#[cfg(not(unix))]
pub(crate) fn is_empty_dir(_: &File) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(not(unix))]
pub(crate) fn set_dir_mode(_: &File, _: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(not(unix))]
pub(crate) fn open_dir(_: &Path, _: bool, _: u32) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "race-resistant private paths are unavailable on this platform",
    ))
}

#[cfg(not(unix))]
pub(crate) fn open_dir_with_status(_: &Path, _: bool, _: u32) -> io::Result<(File, bool)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "race-resistant private paths are unavailable on this platform",
    ))
}

#[cfg(unix)]
pub(crate) fn open_parent(path: &Path, create: bool) -> io::Result<(File, OsString)> {
    let leaf = path
        .file_name()
        .filter(|name| *name != OsStr::new(".") && *name != OsStr::new(".."))
        .ok_or_else(invalid_path)?
        .to_os_string();
    let parent = path.parent().ok_or_else(invalid_path)?;
    Ok((open_dir(parent, create, 0o700)?, leaf))
}

#[cfg(unix)]
pub(crate) fn ensure_dir_at(parent: &File, leaf: &OsStr, mode: u32) -> io::Result<File> {
    let directory = match open_directory_at(parent.as_raw_fd(), leaf) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            create_or_open_directory_at(parent.as_raw_fd(), leaf, mode)?.0
        }
        Err(error) => return Err(error),
    };
    chmod_fd(&directory, mode)?;
    Ok(directory)
}

#[cfg(not(unix))]
pub(crate) fn ensure_dir_at(_: &File, _: &OsStr, _: u32) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(not(unix))]
pub(crate) fn open_parent(_: &Path, _: bool) -> io::Result<(File, OsString)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(unix)]
fn open_file_at(parent: &File, leaf: &OsStr, create: bool, truncate: bool) -> io::Result<File> {
    let name = as_c_string(leaf)?;
    let mut flags = libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    if create {
        flags |= libc::O_CREAT;
    }
    if truncate {
        flags |= libc::O_TRUNC;
    }
    // SAFETY: name is valid and parent is an open directory fd.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    file_from_fd(fd)
}

#[cfg(unix)]
pub(crate) fn create_new_at(parent: &File, leaf: &OsStr) -> io::Result<File> {
    let name = as_c_string(leaf)?;
    // SAFETY: name is valid and parent is an open directory fd.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    file_from_fd(fd)
}

#[cfg(not(unix))]
pub(crate) fn create_new_at(_: &File, _: &OsStr) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(unix)]
pub(crate) fn open_existing_at(parent: &File, leaf: &OsStr) -> io::Result<Option<File>> {
    match open_file_at(parent, leaf, false, false) {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
            }
            Ok(Some(file))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
pub(crate) fn open_existing_at(_: &File, _: &OsStr) -> io::Result<Option<File>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(unix)]
pub(crate) fn ensure_private_file_at(
    parent: &File,
    leaf: &OsStr,
    bytes: &[u8],
) -> io::Result<File> {
    match open_file_at(parent, leaf, false, false) {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
            }
            chmod_fd(&file, 0o600)?;
            Ok(file)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let file = open_file_at(parent, leaf, true, false)?;
            chmod_fd(&file, 0o600)?;
            (&file).write_all(bytes)?;
            file.sync_all()?;
            Ok(file)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
pub(crate) fn ensure_private_file_at(_: &File, _: &OsStr, _: &[u8]) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(unix)]
pub(crate) fn read_private_at(parent: &File, leaf: &OsStr, max: u64) -> io::Result<Vec<u8>> {
    let file = open_existing_at(parent, leaf)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "private file does not exist"))?;
    chmod_fd(&file, 0o600)?;
    if file.metadata()?.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private file is too large",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private file is too large",
        ));
    }
    Ok(bytes)
}

#[cfg(not(unix))]
pub(crate) fn read_private_at(_: &File, _: &OsStr, _: u64) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(unix)]
pub(crate) fn open_lock_at(parent: &File, leaf: &OsStr) -> io::Result<File> {
    let file = match open_file_at(parent, leaf, false, false) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            open_file_at(parent, leaf, true, false)?
        }
        Err(error) => return Err(error),
    };
    chmod_fd(&file, 0o600)?;
    Ok(file)
}

#[cfg(not(unix))]
pub(crate) fn open_lock_at(_: &File, _: &OsStr) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(unix)]
pub(crate) fn rename_at(parent: &File, from: &OsStr, to: &OsStr) -> io::Result<()> {
    let from = as_c_string(from)?;
    let to = as_c_string(to)?;
    // SAFETY: both names are valid and parent remains an open directory.
    let result = unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
pub(crate) fn unlink_at(parent: &File, leaf: &OsStr) -> io::Result<()> {
    unlink_at_fd(parent.as_raw_fd(), leaf)
}

#[cfg(unix)]
fn unlink_at_fd(parent: libc::c_int, leaf: &OsStr) -> io::Result<()> {
    unlink_at_fd_with_flags(parent, leaf, 0)
}

#[cfg(unix)]
fn unlink_at_fd_with_flags(
    parent: libc::c_int,
    leaf: &OsStr,
    flags: libc::c_int,
) -> io::Result<()> {
    let leaf = as_c_string(leaf)?;
    // SAFETY: leaf is valid and parent remains an open directory.
    let result = unsafe { libc::unlinkat(parent, leaf.as_ptr(), flags) };
    let error = io::Error::last_os_error();
    if result == 0 || error.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error)
    }
}

/// Preserve a failed staging directory instead of removing it by pathname.
///
/// There is no portable descriptor-bound directory removal primitive. Even an
/// inode comparison immediately before `unlinkat` leaves a check/use window in
/// which a same-uid process could substitute an unrelated empty directory. A
/// failed creation is therefore fail-closed: its private, random-named staging
/// directory remains for later operator cleanup rather than risking deletion
/// of a replacement pathname.
#[cfg(unix)]
fn preserve_staging_directory(
    _parent: libc::c_int,
    _leaf: &OsStr,
    _identity: Option<(libc::dev_t, libc::ino_t)>,
) {
}

#[cfg(not(unix))]
pub(crate) fn rename_at(_: &File, _: &OsStr, _: &OsStr) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(not(unix))]
pub(crate) fn unlink_at(_: &File, _: &OsStr) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unsupported platform",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mooshik-secure-path-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[test]
    fn creation_replacement_between_stage_mkdir_and_open_fails_closed() {
        let parent_path = test_root();
        let root = parent_path.join("created-home");
        let _ = fs::remove_dir_all(&parent_path);
        fs::create_dir(&parent_path).unwrap();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let parent = options.open(&parent_path).unwrap();
        let name = root.file_name().unwrap();
        let result = create_or_open_directory_at_with_hooks(
            parent.as_raw_fd(),
            name,
            0o700,
            |_| {
                // This is the adversarial scheduling point: an ordinary
                // replacement arrives before the staging directory is opened
                // and installed. The no-replace install must reject it.
                fs::create_dir(&root).map_err(io::Error::other)
            },
            |_| Ok(()),
        );
        assert!(matches!(
            result,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert!(root.is_dir());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
        assert!(fs::read_dir(&parent_path)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".mooshik-stage-")));
        drop(parent);
        let _ = fs::remove_dir_all(&parent_path);
    }

    #[test]
    fn staging_replacement_after_open_before_install_fails_closed() {
        let parent_path = test_root();
        let root = parent_path.join("created-home");
        let moved = parent_path.join("moved-stage");
        let _ = fs::remove_dir_all(&parent_path);
        fs::create_dir(&parent_path).unwrap();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let parent = options.open(&parent_path).unwrap();
        let name = root.file_name().unwrap();
        let result = create_or_open_directory_at_with_hooks(
            parent.as_raw_fd(),
            name,
            0o700,
            |_| Ok(()),
            |temporary| {
                // This is exactly the vulnerable window: the checked/opened
                // source is moved away and an ordinary replacement gets the
                // source pathname before renameat2.
                let temporary_path = parent_path.join(temporary);
                fs::rename(&temporary_path, &moved).map_err(io::Error::other)?;
                fs::create_dir(&temporary_path).map_err(io::Error::other)
            },
        );
        assert!(matches!(
            result,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied
        ));
        assert!(root.is_dir());
        assert!(moved.is_dir());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
        assert!(fs::read_dir(&moved).unwrap().next().is_none());
        drop(parent);
        let _ = fs::remove_dir_all(&parent_path);
    }

    #[test]
    fn staging_directory_is_preserved_after_injected_failure() {
        let parent_path = test_root();
        let root = parent_path.join("created-home");
        let _ = fs::remove_dir_all(&parent_path);
        fs::create_dir(&parent_path).unwrap();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let parent = options.open(&parent_path).unwrap();
        let name = root.file_name().unwrap();
        let result = create_or_open_directory_at_with_hooks(
            parent.as_raw_fd(),
            name,
            0o700,
            |_| Err(io::Error::other("injected staging failure")),
            |_| Ok(()),
        );
        assert!(result.is_err());
        assert!(!root.exists());
        assert!(fs::read_dir(&parent_path)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".mooshik-stage-")));
        drop(parent);
        let _ = fs::remove_dir_all(&parent_path);
    }

    #[test]
    fn staging_directory_replacement_between_snapshot_and_open_fails_closed() {
        let parent_path = test_root();
        let root = parent_path.join("created-home");
        let moved = parent_path.join("moved-stage");
        let _ = fs::remove_dir_all(&parent_path);
        fs::create_dir(&parent_path).unwrap();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let parent = options.open(&parent_path).unwrap();
        let name = root.file_name().unwrap();
        let result = create_or_open_directory_at_with_hooks(
            parent.as_raw_fd(),
            name,
            0o700,
            |temporary| {
                let temporary_path = parent_path.join(temporary);
                fs::rename(&temporary_path, &moved).map_err(io::Error::other)?;
                fs::create_dir(&temporary_path).map_err(io::Error::other)
            },
            |_| Ok(()),
        );
        assert!(matches!(
            result,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied
        ));
        assert!(!root.exists());
        assert!(moved.is_dir());
        drop(parent);
        let _ = fs::remove_dir_all(&parent_path);
    }

    #[test]
    fn creation_protocol_reports_success_only_for_our_mkdir() {
        let parent_path = test_root();
        let root = parent_path.join("existing-home");
        let _ = fs::remove_dir_all(&parent_path);
        fs::create_dir(&parent_path).unwrap();
        fs::create_dir(&root).unwrap();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let parent = options.open(&parent_path).unwrap();
        let name = root.file_name().unwrap();
        let (directory, created) =
            create_or_open_directory_at(parent.as_raw_fd(), name, 0o700).unwrap();
        assert!(!created);
        drop(directory);
        drop(parent);
        let _ = fs::remove_dir_all(&parent_path);
    }
}

use std::io::{Read, Write};
