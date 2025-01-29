use x11::xlib;

/// returns: ((width, height), data)
pub fn screenshot() -> ((u32, u32), Vec<u8>) {
    let display = unsafe { xlib::XOpenDisplay(std::ptr::null()) };
    let screen = unsafe { xlib::XDefaultScreen(display) };
    let root = unsafe { xlib::XRootWindow(display, screen) };

    let height = unsafe { xlib::XDisplayHeight(display, screen) };
    let width = unsafe { xlib::XDisplayWidth(display, screen) };

    let image = unsafe {
        xlib::XGetImage(
            display,
            root,
            0,
            0,
            width as _,
            height as _,
            xlib::XAllPlanes(),
            xlib::ZPixmap,
        )
    };

    assert!(!image.is_null());

    let visual = unsafe { xlib::XDefaultVisual(display, screen) };
    let (red_mask, green_mask, blue_mask) = unsafe {
        let v = *visual;
        (v.red_mask, v.green_mask, v.blue_mask)
    };

    const CHANNELS: usize = 4;

    let mut buf: Vec<u8> = vec![0; (width * height) as usize * CHANNELS];
    for y in 0..height {
        for x in 0..width {
            let pixel = unsafe { xlib::XGetPixel(image, x, y) };

            let rgb: [u8; CHANNELS] = [
                ((pixel & red_mask) >> red_mask.trailing_zeros()) as _,
                ((pixel & green_mask) >> green_mask.trailing_zeros()) as _,
                ((pixel & blue_mask) >> blue_mask.trailing_zeros()) as _,
                255,
            ];

            // Calculate the index for the flipped image
            let index = ((height - 1 - y) * width + x) as usize * CHANNELS;
            // let index = (y * width + x) as usize * CHANNELS;
            buf[index..index + CHANNELS].copy_from_slice(&rgb);
        }
    }

    unsafe {
        xlib::XDestroyImage(image);
        xlib::XCloseDisplay(display);
    }

    ((width as _, height as _), buf)
}
