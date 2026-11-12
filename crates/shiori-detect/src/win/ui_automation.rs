//! A browser's page title, address and tab names, through UI Automation.
//! A port of Anisthesia's `win_ui_automation.cpp`.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::{VT_BOOL, VariantClear};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
    IUIAutomationValuePattern, UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId,
    UIA_IsValuePatternAvailablePropertyId, UIA_MenuBarControlTypeId, UIA_PROPERTY_ID,
    UIA_TabControlTypeId, UIA_TabItemControlTypeId, UIA_TitleBarControlTypeId,
    UIA_ValueIsReadOnlyPropertyId, UIA_ValuePatternId,
};

pub struct Automation {
    automation: IUIAutomation,
    walker: IUIAutomationTreeWalker,
}

#[derive(Debug, Default)]
pub struct Page {
    pub title: String,
    pub address: String,
    pub tabs: Vec<String>,
}

impl Automation {
    pub fn new() -> Option<Self> {
        // SAFETY: plain COM setup for this thread. An error here, such as
        // the thread already using another apartment model, still leaves
        // COM usable, so it is ignored as in Anisthesia.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        // SAFETY: plain COM call; the result is reference counted.
        let automation: IUIAutomation =
            match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
                Ok(automation) => automation,
                Err(error) => {
                    tracing::warn!("UI Automation is not available: {error}");
                    return None;
                }
            };
        // SAFETY: as above.
        let walker = unsafe { automation.ControlViewWalker() }.ok()?;
        Some(Self { automation, walker })
    }

    pub fn browser(&self, hwnd: HWND) -> Option<Page> {
        // SAFETY: COM calls on objects we hold; a window that closed makes
        // them fail, which ends the walk.
        unsafe {
            let window = self.automation.ElementFromHandle(hwnd).ok()?;
            let mut page = Page {
                title: window
                    .CurrentName()
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                ..Page::default()
            };
            if visit(&window, &mut page) {
                self.walk(&window, &mut page, 0);
            }
            Some(page)
        }
    }

    /// Walks the tree by hand instead of with `FindFirst` and `FindAll`, so
    /// that the page itself, which can have thousands of elements, is
    /// skipped.
    unsafe fn walk(&self, parent: &IUIAutomationElement, page: &mut Page, depth: usize) {
        // Deep enough for every browser's toolbar.
        const MAX_DEPTH: usize = 16;
        if depth > MAX_DEPTH {
            return;
        }
        // SAFETY: COM calls on objects we hold.
        unsafe {
            let mut child = self.walker.GetFirstChildElement(parent).ok();
            while let Some(element) = child {
                if visit(&element, page) {
                    self.walk(&element, page, depth + 1);
                }
                child = self.walker.GetNextSiblingElement(&element).ok();
            }
        }
    }
}

/// Reads what `element` holds and says whether to look inside it.
unsafe fn visit(element: &IUIAutomationElement, page: &mut Page) -> bool {
    // SAFETY: COM calls on an element we hold.
    unsafe {
        let kind = element.CurrentControlType().unwrap_or_default();
        match kind {
            // Nothing needed inside these, and skipping documents is what
            // makes this fast on heavy pages.
            k if k == UIA_DocumentControlTypeId
                || k == UIA_MenuBarControlTypeId
                || k == UIA_TitleBarControlTypeId =>
            {
                false
            }
            // The first editable, focusable text field is taken for the
            // address bar. Its name differs between browsers and languages;
            // only the value matters. Opera nests one edit field inside
            // another, so look inside the ones that do not fit.
            k if k == UIA_ComboBoxControlTypeId || k == UIA_EditControlTypeId => {
                if page.address.is_empty() && is_address_bar(element) {
                    page.address = value(element).unwrap_or_default();
                    false
                } else {
                    true
                }
            }
            k if k == UIA_TabControlTypeId => {
                page.tabs.is_empty() && has(element, &[(UIA_ValueIsReadOnlyPropertyId, true)])
            }
            k if k == UIA_TabItemControlTypeId => {
                if let Ok(name) = element.CurrentName() {
                    page.tabs.push(name.to_string());
                }
                false
            }
            // Keep looking until both the address and the tabs are found.
            _ => page.address.is_empty() || page.tabs.is_empty(),
        }
    }
}

unsafe fn is_address_bar(element: &IUIAutomationElement) -> bool {
    // SAFETY: COM calls on an element we hold.
    unsafe {
        has(
            element,
            &[
                (UIA_IsEnabledPropertyId, true),
                (UIA_IsKeyboardFocusablePropertyId, true),
                (UIA_IsValuePatternAvailablePropertyId, true),
                (UIA_ValueIsReadOnlyPropertyId, false),
            ],
        )
    }
}

/// Whether each boolean property has the expected value. A property the
/// element does not have counts as false, as in Anisthesia.
unsafe fn has(element: &IUIAutomationElement, properties: &[(UIA_PROPERTY_ID, bool)]) -> bool {
    properties.iter().all(|&(property, expected)| {
        // SAFETY: COM call on an element we hold; the VARIANT is cleared
        // after reading, which frees anything it holds.
        unsafe {
            let Ok(mut variant) = element.GetCurrentPropertyValue(property) else {
                return false;
            };
            let inner = &variant.Anonymous.Anonymous;
            let actual = inner.vt == VT_BOOL && inner.Anonymous.boolVal.as_bool();
            let _ = VariantClear(&mut variant);
            actual == expected
        }
    })
}

unsafe fn value(element: &IUIAutomationElement) -> Option<String> {
    // SAFETY: COM calls on an element we hold.
    unsafe {
        let pattern: IUIAutomationValuePattern =
            element.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
        Some(pattern.CurrentValue().ok()?.to_string())
    }
}
