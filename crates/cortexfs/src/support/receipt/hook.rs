use std::cell::RefCell;
use std::fs::File;
use std::io;

pub type ParkHook = Box<dyn for<'parent, 'name> FnMut(&'parent File, &'name str) -> io::Result<()>>;

thread_local! {
    static PARK_HOOK: RefCell<Option<ParkHook>> = const { RefCell::new(None) };
}

pub fn set_park_hook(hook: Option<ParkHook>) -> Option<ParkHook> {
    PARK_HOOK.with(|slot| slot.replace(hook))
}

pub(super) fn run(parent: &File, name: &str) -> io::Result<()> {
    let Some(mut hook) = PARK_HOOK.with(|slot| slot.borrow_mut().take()) else {
        return Ok(());
    };
    let result = hook(parent, name);
    PARK_HOOK.with(|slot| {
        if slot.borrow().is_none() {
            slot.replace(Some(hook));
        }
    });
    result?;
    Ok(())
}
