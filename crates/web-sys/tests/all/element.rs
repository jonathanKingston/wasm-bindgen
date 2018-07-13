use super::websys_project;

#[test]
fn element() {
    websys_project()
        .file(
            "src/lib.rs",
            r#"
                #![feature(proc_macro, wasm_custom_section)]
                extern crate wasm_bindgen;
                use wasm_bindgen::prelude::*;
                extern crate web_sys;

                #[wasm_bindgen]
                pub fn test_element(element: &web_sys::Element) {
                    assert_eq!(element.get_attribute("id"), "".into());
                    // must_use is set on this result?
                    let _res = element.set_attribute("id", "beep");
                    assert_eq!(element.get_attribute("id"), "beep".into());
                }
            "#,
        )
        .file(
            "test.js",
            r#"
                import * as assert from "assert";
                import * as wasm from "./out";

                export function test() {
                    let document = new Document();
                    let el = document.createElement("div");
                    wasm.test_element(el);
                }
            "#,
        )
        .test();
}
