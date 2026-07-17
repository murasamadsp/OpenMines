use openmines_macros::gui;

#[test]
fn gui_transpiles_to_the_real_horb_wire_document() {
    let name = "Alice";
    let enabled = true;
    let credits = 7;
    let dynamic_value = 0xff;
    let dynamic_label = 1.0;
    let crystal_lines = vec!["0:0:0:0:line".to_owned(); 6];
    let markers = vec![(10, 10, "tp:10:10".to_owned())];

    let horb = gui! {
        <window title="Title" style="window-css">
            <text>"Hi, " {name} "!"</text>
            <input placeholder="Name" focus-console=true />
            <card value="i0:TP" />
            <inventory value="0: 2;!" />
            <admin />
            <crystals left="L" right="R" buy=true lines=crystal_lines.clone() />
            <tabs>
                <tab label="Info" active=true />
                <tab label="Admin" action="admin" />
            </tabs>
            <list><row title="Row" subtitle="More" action="open" /></list>
            <form>
                <text-row label="Form" />
                <toggle-row label="Enabled" key="enabled" active=enabled />
                <uint-row label="Credits" key="credits" value=credits />
                <button-row label="Action" btn-label="Run" action="run" />
                <dropdown-row label="Static" key="static" selected=1>
                    <option value=0 label="Zero" />
                    <option value=1 label="One" />
                </dropdown-row>
                <dropdown-row label="Dynamic" key="dynamic">
                    <option value=dynamic_value label=dynamic_label />
                </dropdown-row>
            </form>
            <canvas style="canvas-css">
                <rect x=1 y=2 w=3 h=4 color="ff0000" />
                <teleport-point x=5 y=6 action="teleport" />
            </canvas>
            <minimap center-x=0 center-y=0 radius=0 cell-empty={|_, _| Some(true)} markers=markers />
            <buttons><button label="Close" action="close" /></buttons>
            <close-button />
        </window>
    };

    let json = horb.to_json();
    assert_eq!(json["title"], "Title");
    assert_eq!(json["text"], "Hi, Alice!");
    assert_eq!(json["css"], "canv-w=36;canv-h=36");
    assert_eq!(json["input_place"], "Name");
    assert_eq!(json["input_console"], true);
    assert_eq!(json["card"], "i0:TP");
    assert_eq!(json["inv"], "0: 2;!");
    assert_eq!(json["admin"], true);
    assert_eq!(json["crys_lines"], serde_json::json!(crystal_lines));
    assert_eq!(
        json["tabs"],
        serde_json::json!(["Info", "", "Admin", "admin"])
    );
    assert_eq!(json["list"], serde_json::json!(["Row", "More", "open"]));
    assert_eq!(json["richList"][22], "0:Zero#1:One#");
    assert_eq!(json["richList"][27], "255:1#");
    assert_eq!(
        json["canvas"],
        serde_json::json!([
            "1X2Y3w4h=R#ff0000",
            "5X6Y=t",
            "teleport",
            "0X0Y18w18h=R#008000",
            "0X0Y18w18h=R#ff3030",
            "0X0Y=t",
            "tp:10:10",
        ])
    );
    assert_eq!(
        json["buttons"],
        serde_json::json!(["Close", "close", "ВЫЙТИ", "exit"])
    );
}

#[test]
fn gui_supports_runtime_conditions_and_repeated_sections() {
    let visible = true;
    let actions = [("One", "one"), ("Two", "two")];

    let horb = gui! {
        <window title="Dynamic">
            <if condition=visible><text>"Visible"</text></if>
            <for each={actions} item=action>
                <buttons><button label=action.0 action=action.1 /></buttons>
            </for>
        </window>
    };

    assert_eq!(
        horb.to_json()["buttons"],
        serde_json::json!(["One", "one", "Two", "two", "ВЫЙТИ", "exit"])
    );
}
