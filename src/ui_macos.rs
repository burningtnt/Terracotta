use objc2::{
    ClassType, class, define_class,
    ffi::nil,
    msg_send,
    runtime::{AnyObject, Bool},
};
use objc2_app_kit::{NSBackingStoreType, NSWindowStyleMask};
use objc2_foundation::{NSAutoreleasePool, NSObject, NSPoint, NSRect, NSSize, NSString};
#[allow(unused_imports)]
use objc2_web_kit::{WKNavigationDelegate, WKWebView, WKWebViewConfiguration};

define_class!(
    #[unsafe(super(NSObject))]
    struct AppDelegate;

    impl AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _: *mut AnyObject) {
            unsafe {
                let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                let _: () = msg_send![app, terminate:app];
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    struct WebviewObserver;

    impl WebviewObserver {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn observer_value(&self, webview: *mut AnyObject, _: *mut AnyObject) {
            unsafe {
                delegate_display(webview);
            }
        }
    }
);

unsafe fn delegate_display(webview: *mut AnyObject) {
    use std::sync::atomic::{AtomicPtr, Ordering::Acquire};
    use std::thread;

    let webview = AtomicPtr::new(webview);
    thread::spawn(move || {
        let webview: *mut AnyObject = webview.load(Acquire);

        loop {
            let title: *mut NSString = msg_send![webview, title];
            if objc2::rc::autoreleasepool(|pool| {
                unsafe {
                    return NSString::to_str(&*title, pool).contains("Terracotta");
                }
            }) {
                thread::sleep(std::time::Duration::from_millis(200));
                let _: () = msg_send![webview, setHidden: Bool::NO];
                return;
            }
        }
    });
}

pub fn open(url: String) {
    unsafe {
        let _pool = NSAutoreleasePool::new();

        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: Bool = msg_send![app, setActivationPolicy: 0i64];

        // Setup Window
        let window: *mut AnyObject = msg_send![class!(NSWindow), alloc];
        let frame = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1000., 700.));
        let window: *mut AnyObject = msg_send![
            window,
            initWithContentRect: frame,
            styleMask: NSWindowStyleMask::Resizable | NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            backing: NSBackingStoreType::Buffered,
            defer: Bool::NO
        ];

        let min_size = NSSize::new(500., 500.);
        let _: () = msg_send![window, setMinSize: min_size];

        let _: () = msg_send![window, setTitle: &*NSString::from_str("Terracotta | 陶瓦联机")];

        // Setup WebView
        let config: *mut AnyObject = msg_send![class!(WKWebViewConfiguration), new];
        let webview: *mut AnyObject = msg_send![class!(WKWebView), alloc];
        let webview: *mut AnyObject = msg_send![webview, initWithFrame:frame, configuration:config];

        let color: *mut AnyObject =
            msg_send![class!(NSColor), colorWithRed:0.102, green:0.102, blue:0.18, alpha:1.0];
        let _: () = msg_send![window, setBackgroundColor: color];
        let _: () = msg_send![webview, setBackgroundColor: color];
        let _: () = msg_send![webview, setHidden: Bool::YES];

        // Load URL
        let url_str = NSString::from_str(&url);
        let url: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*url_str];
        let request: *mut AnyObject = msg_send![class!(NSURLRequest), requestWithURL: url];
        let _: *mut AnyObject = msg_send![webview, loadRequest: request];

        // Add observer for page load completion
        let observer: *mut AnyObject = msg_send![WebviewObserver::class(), new];
        let _: () = msg_send![webview, setNavigationDelegate: observer];

        // Bind WebView to window
        let content_view: *mut AnyObject = msg_send![window, contentView];
        let _: () = msg_send![content_view, addSubview:webview];
        let _: () = msg_send![webview, setAutoresizingMask: 18u64];

        // Delegate for window close
        let delegate: *mut AnyObject = msg_send![AppDelegate::class(), new];
        let _: () = msg_send![window, setDelegate:delegate];

        let _: () = msg_send![window, makeKeyAndOrderFront:nil];

        let _: () = msg_send![app, activateIgnoringOtherApps:Bool::YES];

        // Run the app
        let _: () = msg_send![app, run];
    }
}
