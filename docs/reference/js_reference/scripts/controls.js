let options_ignore_borders = document.getElementById("options_ignore_borders");
let options_vectors = document.getElementById("options_vectors");
let options_nicknames = document.getElementById("options_nicknames");
let options_fps = document.getElementById("options_fps");
let options_prog_ticks = document.getElementById("options_prog_ticks");
let options_view_box = document.getElementById("options_view_box");

options_ignore_borders.checked = WOptions.ignoreBorders;
options_vectors.checked = WOptions.vectors;
options_nicknames.checked = WOptions.nicknames;
options_fps.value = WOptions.fps;
options_prog_ticks.value = WOptions.progTicks;
options_view_box.checked = WOptions.viewBox;

const KeyFlags = {
    w: false,
    a: false,
    s: false,
    d: false,
    z: false,
    delta: 0,
    shift: false,
    ctrl: false,
    timePrev: 0,
    mouse: [0, 0, 0],
    mousepos: new Vector2d(0, 0),
    Reset(){
        this.w = this.a = this.s = this.d = this.z = this.shift = this.ctrl = false;
        this.delta = 0;
    }
}

window.addEventListener('blur', (event) => {
    KeyFlags.Reset();
});

options_view_box.onclick = (event) => {
    WOptions.viewBox = options_view_box.checked;
};

options_ignore_borders.onclick = (event) => {
    WOptions.ignoreBorders = options_ignore_borders.checked;
};

options_vectors.onclick = (event) => {
    WOptions.vectors = options_vectors.checked;
};

options_nicknames.onclick = (event) => {
    WOptions.nicknames = options_nicknames.checked;
};

options_fps.oninput = (event) => {
    WOptions.fps = event.target.value;
};

options_prog_ticks.oninput = (event) => {
    WOptions.progTicks = event.target.value;
};

WorldCanv.addEventListener("mousedown", (event) => {
    KeyFlags.mouse[event.button] = true;
    let [x, y] = [event.offsetX, event.offsetY];
    if (KeyFlags.mouse[0]) {
        WRender.MouseSetBlock(x, y, 48);
    }
    if (KeyFlags.mouse[1]) {
        WRender.MouseSetBlock(x, y);
    }
});

WorldCanv.addEventListener("mousemove", (event) => {
    let [x, y] = [event.offsetX, event.offsetY];
    if (KeyFlags.mouse[0]) {
        WRender.MouseSetBlock(x, y, 48);
    }
    if (KeyFlags.mouse[1]) {
        WRender.MouseSetBlock(x, y, null, 5);
    }
    [KeyFlags.mousepos.x, KeyFlags.mousepos.y] = [x, y];
});

WorldCanv.addEventListener("mouseleave", (event) => {
    [KeyFlags.mouse[0], KeyFlags.mouse[1], KeyFlags.mouse[2]] = [false];
});

WorldCanv.addEventListener("mouseup", (event) => {
    KeyFlags.mouse[event.button] = false;
});


setInterval(() => {
    let speed = 1;
    KeyFlags.delta = performance.now() - KeyFlags.timePrev;
    if (KeyFlags.w) {
        if (KeyFlags.shift) {
            WRender.CurrentBot.Rotate("u",true);
        } else {
            while (KeyFlags.delta > 0) {
                KeyFlags.delta -= WRender.CurrentBot.Move("u",true);
            }

        }
    }
    else if (KeyFlags.d) {
        if (KeyFlags.shift) {
            WRender.CurrentBot.Rotate("r",true);
        } else {
            while (KeyFlags.delta > 0) {
                KeyFlags.delta -= WRender.CurrentBot.Move("r",true);
            }
        }

    }
    else if (KeyFlags.s) {
        if (KeyFlags.shift) {
            WRender.CurrentBot.Rotate("d",true);
        } else {
            while (KeyFlags.delta > 0) {
                KeyFlags.delta -= WRender.CurrentBot.Move("d",true)
            }
        }

    }
    else if (KeyFlags.a) {
        if (KeyFlags.shift) {
            WRender.CurrentBot.Rotate("l",true);
        } else {
            while (KeyFlags.delta > 0) {
                KeyFlags.delta -= WRender.CurrentBot.Move("l",true);
            }
        }
    }
    else if (KeyFlags.z) {
        WRender.CurrentBot.Digg(true);
    }
}, 1);

document.addEventListener("keydown", (event) => {
    //console.log(event,event.target.type);
    if(event.target.tagName == "INPUT" && event.target.type == "text") return;
    switch (event.code) {

        case "KeyW":
        case "ArrowUp":
            if (!KeyFlags.w) {
                KeyFlags.delta = 0; KeyFlags.timePrev = performance.now();
                KeyFlags.w = true;
            }
            break;
        case "ArrowRight":
        case "KeyD":
            if (!KeyFlags.d) {
                KeyFlags.delta = 0; KeyFlags.timePrev = performance.now();
                KeyFlags.d = true;
            }
            break;
        case "KeyS":
        case "ArrowDown":
            if (!KeyFlags.s) {
                KeyFlags.delta = 0; KeyFlags.timePrev = performance.now();
                KeyFlags.s = true;
            }
            break;

        case "KeyA":
        case "ArrowLeft":
            if (!KeyFlags.a) {
                KeyFlags.delta = 0; KeyFlags.timePrev = performance.now();
                KeyFlags.a = true;
            }
            break;
        case "ShiftLeft":
        case "ShiftRight":
            KeyFlags.shift = true;
            break;
        case "ControlLeft":
        case "ControlRight":
            KeyFlags.ctrl = true;
            if(WRender.CurrentBot){WRender.CurrentBot.mode.roadignore = !WRender.CurrentBot.mode.roadignore}
            break;
        case "KeyF":
            WRender.CurrentBot.SetBlock(true);
            break;
        case "KeyV":
            WRender.CurrentBot.Heal(true);
            break;
        case "Space":
            let newbot = WRender.Bots[Math.floor(Math.random() * WRender.Bots.length)];
            WRender.CurrentBot = newbot;
            break
        case "KeyJ":
            WRender.CurrentBot.SetQuadro(true);
            break
        case "KeyH":
            WRender.CurrentBot.SetRoad(true);
            break
        case "KeyZ":
            KeyFlags.z = true;
            break
        case "KeyY":
            WRender.CurrentBot.SetWB(true);
            break
        case "KeyG":
            WRender.CurrentBot.UseGeo(true);
            break
            case "KeyM":
            OpenMap();
            break
        case "KeyE":
            WRender.CurrentBot.mode.autodigg = !WRender.CurrentBot.mode.autodigg;
            break
        case "KeyR":
            if (KeyFlags.ctrl) { event.preventDefault() }////////////////////////////////////////////////////////////////Выключение комбы для рефреша страницы
            if (KeyFlags.shift) {
                for (let i = 0; i < WRender.Bots.length; i++) {
                    WRender.Bots[i].programCondition.isExecute = !WRender.Bots[i].programCondition.isExecute;
                    if(!WRender.Bots[i].programCondition.isExecute){WRender.Bots[i].programCondition.Reset()}
                }
            }
            else {
                WRender.CurrentBot.programCondition.isExecute = !WRender.CurrentBot.programCondition.isExecute;
                if(!WRender.CurrentBot.programCondition.isExecute){WRender.CurrentBot.programCondition.Reset()}
            }
            break
        case "KeyC":
            for (let i = 0; i < WRender.Bots.length; i++) {
                if (WRender.Bots[i].id != WRender.CurrentBot.id) {
                    if(KeyFlags.ctrl){
                        WRender.Bots[i].Teleport(WRender.CurrentBot.position.x + Math.round((Math.random() - 0.5) * 10),WRender.CurrentBot.position.y + Math.round((Math.random() - 0.5) * 10))
                    }else{
                        WRender.Bots[i].Teleport(WRender.CurrentBot.position.x,WRender.CurrentBot.position.y)
                    }
                    
                    if(KeyFlags.shift){
                        WRender.Bots[i].rotation = RandInt(0, 3) * 90;
                    }
                    else{WRender.Bots[i].rotation = WRender.CurrentBot.rotation;}
                }
            }
            break
        default:
            //console.log(event.code,event.key);
            break;

    }
})

document.addEventListener("keyup", (event) => {
    //console.log(event);
    switch (event.code) {
        case "KeyZ":
            KeyFlags.z = false;
            break
        case "KeyW":
        case "ArrowUp":
            KeyFlags.timePrev = performance.now();
            KeyFlags.w = false;
            break;
        case "ArrowRight":
        case "KeyD":
            KeyFlags.timePrev = performance.now();
            KeyFlags.d = false;
            break;
        case "KeyS":
        case "ArrowDown":
            KeyFlags.timePrev = performance.now();
            KeyFlags.s = false;
            break;
        case "KeyA":
        case "ArrowLeft":
            KeyFlags.timePrev = performance.now();
            KeyFlags.a = false;
            break;
        case "ShiftLeft":
        case "ShiftRight":
            KeyFlags.shift = false;
            break;
        case "ControlLeft":
        case "ControlRight":
            KeyFlags.ctrl = false;
            break;
        default:
            //console.log(event.code);
            break;

    }
})