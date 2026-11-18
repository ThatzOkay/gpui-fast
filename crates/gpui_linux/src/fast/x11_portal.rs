pub(crate) fn window_identifier(
    client: &crate::linux::X11Client,
) -> std::future::Ready<Option<ashpd::WindowIdentifier>> {
    let state = client.0.borrow();
    let identifier = state
        .keyboard_focused_window
        .and_then(|focused_window| state.windows.get(&focused_window))
        .map(|window| ashpd::WindowIdentifier::from_xid(window.window.x_window as _));
    std::future::ready(identifier)
}
