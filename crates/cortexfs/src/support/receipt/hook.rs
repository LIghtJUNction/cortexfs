use std::cell::RefCell;
use std::fs::File;
use std::io;

pub type ParkHook =
    Box<dyn for<'parent, 'name> FnOnce(&'parent File, &'name str) -> io::Result<()>>;

thread_local! {
    static PARK_HOOK: RefCell<Option<ParkHook>> = const { RefCell::new(None) };
}

pub fn set_park_hook(hook: Option<ParkHook>) -> Option<ParkHook> {
    PARK_HOOK.with(|slot| slot.replace(hook))
}

pub(super) fn run(parent: &File, name: &str) -> io::Result<()> {
    if let Some(hook) = PARK_HOOK.with(|slot| slot.borrow_mut().take()) {
        hook(parent, name)?;
    }
    Ok(())
}
