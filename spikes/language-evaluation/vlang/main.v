module main

#flag windows -lole32
#flag windows -loleaut32
#flag windows -luuid
#flag windows -luser32
#flag windows -lkernel32
#flag windows -Wno-incompatible-pointer-types
#include "windows_probe.c"

fn C.act_probe_process_count() u32
fn C.act_probe_window_count() u32
fn C.act_probe_uia_available() int
fn C.act_probe_foreground() u64

fn json_bool(value bool) string {
	return if value { 'true' } else { 'false' }
}

fn main() {
	foreground_before := C.act_probe_foreground()
	processes := C.act_probe_process_count()
	windows := C.act_probe_window_count()
	uia_initialized := C.act_probe_uia_available() == 1
	foreground_after := C.act_probe_foreground()
	foreground_unchanged := foreground_before == foreground_after
	ok := processes > 0 && windows > 0 && uia_initialized && foreground_unchanged
	println('{"ok":${json_bool(ok)},"contractVersion":"act/language-probe/v1","implementationLanguage":"vlang","platform":"windows","observations":{"processCount":${processes},"topLevelWindowCount":${windows},"uiaClientInitialized":${json_bool(uia_initialized)},"foregroundUnchanged":${json_bool(foreground_unchanged)}},"safety":{"readOnly":true,"inputSent":false,"windowActivated":false}}')
	if !ok {
		exit(2)
	}
}
