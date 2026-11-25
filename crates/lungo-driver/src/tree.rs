//! Removing a directory tree.

use std::io;
use std::path::Path;

/// Removes the directory `path` and everything under it. A symbolic link is removed, never
/// followed: `path` itself when it is one, and every link inside.
///
/// On Unix each directory's listing is read to its end before any of its entries is removed.
/// `std::fs::remove_dir_all` removes each entry as it reads it, and a file system may lose its
/// place in a listing whose entries are removed under it. APFS hands a listing out in batches
/// (2 KiB of entries through `fdopendir`) and resumes each batch from a record it keeps for the
/// directory; once 32 listings of that directory have begun since — anything listing it, such
/// as an indexer or a build reading the sources — the record is gone and the batch resumes at a
/// position, which the removals have moved every remaining entry below. The listing then ends
/// early, the entries after the first batch are never removed, and removing the directory fails
/// with "Directory not empty". A listing read to its end before anything is removed names every
/// entry, whatever else lists the directory meanwhile.
///
/// A directory that still holds entries once everything its listing named is removed was written
/// to while it was removed: that is an error naming the directory and what it holds.
///
/// On Windows the standard library's removal lists a directory again from its start until it is
/// empty, and waits for a delete another process holds open, so it is used as it is.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    imp::remove_tree(path)
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::path::Path;

    pub fn remove_tree(path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }
}

#[cfg(unix)]
mod imp {
    use std::ffi::{CStr, CString, OsStr};
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn remove_tree(path: &Path) -> io::Result<()> {
        if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            return std::fs::remove_file(path);
        }
        let name = c_name(path.as_os_str())?;
        let dir = Dir::open(libc::AT_FDCWD, &name)?;
        remove_contents(&dir, path)?;
        drop(dir);
        remove_dir(libc::AT_FDCWD, &name, path)
    }

    /// A directory opened without following a symbolic link; closed when dropped.
    struct Dir(libc::c_int);

    impl Dir {
        fn open(parent: libc::c_int, name: &CStr) -> io::Result<Dir> {
            let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            let fd = unsafe { libc::openat(parent, name.as_ptr(), flags) };
            if fd < 0 { Err(io::Error::last_os_error()) } else { Ok(Dir(fd)) }
        }

        /// Every entry but `.` and `..`, read to the end of the listing, each with whether it is
        /// a directory (a symbolic link to one is not).
        fn listing(&self) -> io::Result<Vec<(CString, bool)>> {
            // `closedir` closes the descriptor `fdopendir` was given: it is given its own.
            let fd = unsafe { libc::dup(self.0) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let stream = unsafe { libc::fdopendir(fd) };
            if stream.is_null() {
                let err = io::Error::last_os_error();
                unsafe { libc::close(fd) };
                return Err(err);
            }
            let mut entries = Vec::new();
            let read = loop {
                // `readdir` answers null both at the end of the listing and on an error, which
                // it tells apart only by setting errno.
                unsafe { *errno() = 0 };
                let entry = unsafe { libc::readdir(stream) };
                if entry.is_null() {
                    let err = io::Error::last_os_error();
                    break if err.raw_os_error() == Some(0) { Ok(()) } else { Err(err) };
                }
                let entry = unsafe { &*entry };
                let name = unsafe { CStr::from_ptr(entry.d_name.as_ptr()) };
                if name == c"." || name == c".." {
                    continue;
                }
                let is_dir = match entry.d_type {
                    libc::DT_DIR => true,
                    libc::DT_UNKNOWN => match self.is_dir(name) {
                        Ok(is_dir) => is_dir,
                        Err(err) => break Err(err),
                    },
                    _ => false,
                };
                entries.push((name.to_owned(), is_dir));
            };
            unsafe { libc::closedir(stream) };
            read.map(|()| entries)
        }

        fn is_dir(&self, name: &CStr) -> io::Result<bool> {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe { libc::fstatat(self.0, name.as_ptr(), stat.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { stat.assume_init() }.st_mode & libc::S_IFMT == libc::S_IFDIR)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            unsafe { libc::close(self.0) };
        }
    }

    #[cfg(target_vendor = "apple")]
    fn errno() -> *mut libc::c_int {
        unsafe { libc::__error() }
    }

    #[cfg(target_os = "linux")]
    fn errno() -> *mut libc::c_int {
        unsafe { libc::__errno_location() }
    }

    fn c_name(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("{} holds a NUL byte", name.display())))
    }

    /// Removes everything the directory `dir`, at `path`, holds. An entry that is already gone
    /// is not an error: something else removed it.
    fn remove_contents(dir: &Dir, path: &Path) -> io::Result<()> {
        for (name, is_dir) in dir.listing()? {
            let child = path.join(OsStr::from_bytes(name.to_bytes()));
            let removed = if is_dir {
                Dir::open(dir.0, &name)
                    .and_then(|sub| remove_contents(&sub, &child))
                    .and_then(|()| remove_dir(dir.0, &name, &child))
            } else if unsafe { libc::unlinkat(dir.0, name.as_ptr(), 0) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            };
            match removed {
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                removed => removed?,
            }
        }
        Ok(())
    }

    /// Removes the emptied directory `name` of `parent`, at `path`.
    fn remove_dir(parent: libc::c_int, name: &CStr, path: &Path) -> io::Result<()> {
        if unsafe { libc::unlinkat(parent, name.as_ptr(), libc::AT_REMOVEDIR) } == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::DirectoryNotEmpty {
            return Err(err);
        }
        let held = Dir::open(parent, name).and_then(|dir| dir.listing());
        let held = match held {
            Ok(entries) => entries.iter().map(|(n, _)| n.to_string_lossy().into_owned()).collect::<Vec<_>>().join(", "),
            Err(e) => format!("entries it cannot list ({e})"),
        };
        Err(io::Error::new(
            io::ErrorKind::DirectoryNotEmpty,
            format!("{} was written to while it was removed: it holds {held}", path.display()),
        ))
    }
}
