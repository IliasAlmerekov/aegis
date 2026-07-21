#![no_main]

use aegis::analysis::router;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // `router::route` is a pure, filesystem-free function (ADR-022 §6
    // Iteration 4) that parses arbitrary shell command strings, including
    // hand-rolled heredoc-marker splitting and same-command heredoc-to-file
    // reuse detection — fuzz it for panic-freedom over arbitrary byte input.
    let input = String::from_utf8_lossy(data);
    let targets = router::route(input.as_ref(), &[("py", "python3")]);

    for target in &targets {
        match target {
            router::RoutedTarget::Inline { source, .. } => {
                let _ = source.len();
            }
            router::RoutedTarget::ScriptFile { path, .. }
            | router::RoutedTarget::DirectExec { path } => {
                let _ = path.as_os_str().len();
            }
            router::RoutedTarget::Dynamic { reason, .. } => {
                let _ = format!("{reason:?}");
            }
        }
    }

    // Exercised by a real `resolve()` call on a `DirectExec` route — fuzz it
    // directly too, since it also does its own ad hoc string parsing.
    let _ = router::verified_shebang_language(input.as_ref());
});
