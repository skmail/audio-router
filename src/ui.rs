use crate::state::{Assignment, State};
use objc2::{
    AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject, sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSData, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer, NSURL,
};
use std::cell::{OnceCell, RefCell};

#[derive(Clone)]
enum Action {
    Assign(String, Option<Assignment>),
    Pause,
    Retry,
    Reset,
    Settings,
    Quit,
}

#[derive(Default)]
pub struct Ivars {
    status: OnceCell<Retained<NSStatusItem>>,
    menu: OnceCell<Retained<NSMenu>>,
    timer: OnceCell<Retained<NSTimer>>,
    state: RefCell<State>,
    actions: RefCell<Vec<Action>>,
    volume_controls: RefCell<Vec<(String, Retained<NSTextField>)>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self, _: &NSNotification) {
            *self.ivars().state.borrow_mut() = State::load();
            let status = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
            if let Some(button) = status.button(self.mtm()) {
                if let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(include_bytes!("../resources/MenuBarTemplate.png"))) {
                    image.setSize(NSSize::new(18.0, 18.0));
                    image.setTemplate(true); button.setImage(Some(&image));
                } else { button.setTitle(&NSString::from_str("Audio")); }
                button.setToolTip(Some(&NSString::from_str("Audio Router — route apps to speakers or headphones")));
            }
            let menu = NSMenu::new(self.mtm()); menu.setAutoenablesItems(false);
            menu.setDelegate(Some(ProtocolObject::from_ref(self))); status.setMenu(Some(&menu));
            self.ivars().status.set(status).ok(); self.ivars().menu.set(menu).ok();
            self.ivars().state.borrow_mut().refresh(); self.rebuild();
            let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(2.0, self, sel!(tick:), None, true) };
            self.ivars().timer.set(timer).ok();
        }
        #[unsafe(method(applicationWillTerminate:))]
        fn terminating(&self, _: &NSNotification) {
            if let Some(timer) = self.ivars().timer.get() { timer.invalidate(); }
            self.ivars().state.borrow_mut().stop();
        }
    }
    unsafe impl NSMenuDelegate for Delegate {
        #[unsafe(method(menuWillOpen:))]
        fn menu_open(&self, _: &NSMenu) {
            self.ivars().state.borrow_mut().refresh(); self.rebuild();
        }
    }
    impl Delegate {
        #[unsafe(method(changeVolume:))]
        fn change_volume(&self, slider: &NSSlider) {
            let controls = self.ivars().volume_controls.borrow();
            let Some((uid, label)) = controls.get(slider.tag() as usize) else { return; };
            let volume = slider.doubleValue().round() as u8;
            self.ivars().state.borrow_mut().set_volume(uid, volume);
            label.setStringValue(&NSString::from_str(&format!("Routed volume: {volume}%")));
        }
        #[unsafe(method(tick:))]
        fn tick(&self, _: &NSTimer) { self.ivars().state.borrow_mut().refresh(); }
        #[unsafe(method(perform:))]
        fn perform(&self, item: &NSMenuItem) {
            let action = self.ivars().actions.borrow().get(item.tag() as usize).cloned();
            let Some(action) = action else { return; };
            match action {
                Action::Assign(key, assignment) => self.ivars().state.borrow_mut().assign(key, assignment),
                Action::Pause => { let mut state = self.ivars().state.borrow_mut(); state.paused = !state.paused; state.refresh(); }
                Action::Retry => self.ivars().state.borrow_mut().retry(),
                Action::Reset => { let mut state = self.ivars().state.borrow_mut(); state.stop(); state.assignments.clear(); state.save(); state.retry(); }
                Action::Settings => {
                    if let Some(url) = NSURL::URLWithString(&NSString::from_str("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")) { NSWorkspace::sharedWorkspace().openURL(&url); }
                }
                Action::Quit => { self.ivars().state.borrow_mut().stop(); NSApplication::sharedApplication(self.mtm()).terminate(None); }
            }
        }
    }
);

impl Delegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars::default());
        unsafe { msg_send![super(this), init] }
    }
    fn item(
        &self,
        menu: &NSMenu,
        title: &str,
        action: Option<Action>,
        checked: bool,
    ) -> Retained<NSMenuItem> {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm()),
                &NSString::from_str(title),
                None,
                &NSString::new(),
            )
        };
        item.setEnabled(action.is_some());
        if let Some(action) = action {
            let mut actions = self.ivars().actions.borrow_mut();
            item.setTag(actions.len() as isize);
            actions.push(action);
            unsafe {
                item.setTarget(Some(self));
                item.setAction(Some(sel!(perform:)));
            }
        }
        if checked {
            item.setState(NSControlStateValueOn);
        }
        menu.addItem(&item);
        item
    }
    fn separator(&self, menu: &NSMenu) {
        menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
    }
    fn volume_control(&self, menu: &NSMenu, uid: &str, volume: u8) {
        let view = NSView::initWithFrame(
            NSView::alloc(self.mtm()),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(240.0, 58.0)),
        );
        let label = NSTextField::labelWithString(
            &NSString::from_str(&format!("Routed volume: {volume}%")),
            self.mtm(),
        );
        label.setFrame(NSRect::new(
            NSPoint::new(16.0, 32.0),
            NSSize::new(208.0, 20.0),
        ));
        let slider = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                f64::from(volume),
                0.0,
                100.0,
                Some(self),
                Some(sel!(changeVolume:)),
                self.mtm(),
            )
        };
        slider.setFrame(NSRect::new(
            NSPoint::new(16.0, 6.0),
            NSSize::new(208.0, 24.0),
        ));
        slider.setContinuous(true);
        slider.setToolTip(Some(&NSString::from_str(
            "Volume for apps routed to this output. 0% mutes routed audio.",
        )));
        let mut controls = self.ivars().volume_controls.borrow_mut();
        slider.setTag(controls.len() as isize);
        view.addSubview(&label);
        view.addSubview(&slider);
        controls.push((uid.to_owned(), label));
        let item = self.item(menu, "Routed volume", None, false);
        item.setEnabled(true);
        item.setView(Some(&view));
    }
    fn rebuild(&self) {
        let Some(menu) = self.ivars().menu.get() else {
            return;
        };
        menu.removeAllItems();
        self.ivars().volume_controls.borrow_mut().clear();
        self.ivars().actions.borrow_mut().clear();
        let state = self.ivars().state.borrow();
        // Keep the main menu focused on destinations; maintenance lives in Options.
        let mut entries: Vec<(String, String)> = state
            .apps
            .iter()
            .filter(|a| a.playing || state.assignments.contains_key(&a.key))
            .map(|a| (a.key.clone(), a.name.clone()))
            .collect();
        for (key, assignment) in &state.assignments {
            if !entries.iter().any(|e| &e.0 == key) {
                entries.push((key.clone(), assignment.app_name.clone()));
            }
        }
        entries.sort_by(|a, b| a.1.cmp(&b.1));

        for device in &state.devices {
            let parent = self.item(menu, &device.name, None, false);
            parent.setEnabled(true);
            let submenu = NSMenu::new(self.mtm());
            submenu.setAutoenablesItems(false);
            self.volume_control(&submenu, &device.uid, state.volume(&device.uid));
            self.separator(&submenu);
            if entries.is_empty() {
                self.item(&submenu, "Play audio in an app to add it here", None, false);
            }
            for (key, name) in &entries {
                let assignment = state.assignments.get(key);
                let checked = assignment.is_some_and(|a| a.device_uid == device.uid);
                let value = if checked {
                    None
                } else {
                    Some(Assignment {
                        app_name: name.clone(),
                        device_uid: device.uid.clone(),
                        device_name: device.name.clone(),
                    })
                };
                let error = state.errors.get(key).filter(|_| checked);
                let title = if error.is_some() {
                    format!("{name} · needs attention")
                } else {
                    name.clone()
                };
                let item = self.item(
                    &submenu,
                    &title,
                    Some(Action::Assign(key.clone(), value)),
                    checked,
                );
                let hint = if let Some(error) = error {
                    format!(
                        "{error} Use Options to check permission or retry. Click to remove this assignment."
                    )
                } else if checked {
                    "Click to remove this assignment and use the app's normal output.".into()
                } else if let Some(assignment) = assignment {
                    format!("Move from {} to {}.", assignment.device_name, device.name)
                } else {
                    format!("Play this app through {}.", device.name)
                };
                item.setToolTip(Some(&NSString::from_str(&hint)));
            }
            parent.setSubmenu(Some(&submenu));
        }
        if state.devices.is_empty() {
            self.item(menu, "No outputs connected", None, false);
        }

        self.separator(menu);
        let options_item = self.item(
            menu,
            if state.paused {
                "Options · paused"
            } else {
                "Options"
            },
            None,
            false,
        );
        options_item.setEnabled(true);
        let options = NSMenu::new(self.mtm());
        options.setAutoenablesItems(false);
        if let Some(error) = &state.discovery_error {
            self.item(&options, error, None, false);
            self.separator(&options);
        }
        self.item(
            &options,
            if state.paused {
                "Resume routing"
            } else {
                "Pause routing"
            },
            Some(Action::Pause),
            false,
        );
        self.item(
            &options,
            "Refresh / retry routes",
            Some(Action::Retry),
            false,
        );
        self.item(
            &options,
            "Clear all assignments",
            Some(Action::Reset),
            false,
        );
        self.item(
            &options,
            "Audio capture permission…",
            Some(Action::Settings),
            false,
        );

        // Saved destinations remain removable even while their hardware is unplugged.
        let unavailable: Vec<_> = state
            .assignments
            .iter()
            .filter(|(_, a)| !state.devices.iter().any(|d| d.uid == a.device_uid))
            .collect();
        if !unavailable.is_empty() {
            self.separator(&options);
            self.item(
                &options,
                "Disconnected outputs · click an app to unassign",
                None,
                false,
            );
            for (key, assignment) in unavailable {
                self.item(
                    &options,
                    &format!("{} → {}", assignment.app_name, assignment.device_name),
                    Some(Action::Assign(key.clone(), None)),
                    true,
                );
            }
        }
        options_item.setSubmenu(Some(&options));
        self.item(menu, "Quit Audio Router", Some(Action::Quit), false);
    }
}
