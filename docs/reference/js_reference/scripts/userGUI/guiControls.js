//let win = window.Open(() =>"/test.html","_blank",{top:100,left:100,width:100,height:100,resizable:"yes",menubar:"no"})
const windows = {
    programmator: false,
    map: false
}
var map_panel_container = document.getElementById("map_panel_container");
var prog_frame = document.getElementById("prog_frame");

map_panel_container.style.visibility = "hidden";
prog_frame.style.display = "none";

//map_frame.src="/windows/map/map.html";
//prog_frame.src="/windows/programmator/programmator.html";

function resizeCanvas() {
    WorldCanv.width = canv_container.clientWidth;
    WorldCanv.height = canv_container.clientHeight - canv_container.offsetTop;
}

function OpenMap(params) {
    //document.getElementById("myDiv").style.visibility = "hidden";
    map_panel_container.style.visibility = map_panel_container.style.visibility == "hidden" ? "" : "hidden";
    windows.map = map_panel_container.style.visibility == "hidden" ? false : true;
    if (windows.map) { map_panel_container.focus(); MapHasOpened() }
}

function OpenProgrammator(params) {
    if (!prog_frame.src) { prog_frame.src = "/windows/programmator/programmator.html"; }
    prog_frame.style.display = prog_frame.style.display == "block" ? "none" : "block";
    windows.programmator = prog_frame.style.display == "block" ? true : false;
    if (windows.programmator) { prog_frame.focus() }
}

function SendBotProgramStatus(data) {
    if (windows.programmator) {
        prog_frame.contentWindow.postMessage({ comand: "botstatus", data: data }, '*');
    }
}

prog_frame.onload = function () {
    //console.log("prog_frame загрузился");
    prog_frame.contentWindow.postMessage("initprog", '*');
}

function TabSwitch(evt,tabsContainerId, tabId) {
    let conteiner_element = document.getElementById(tabsContainerId);
    let switches = conteiner_element.getElementsByClassName("switch");
    let tabElements = conteiner_element.getElementsByClassName("tab_element");

    //console.log(tabElements);
    //console.log(switches);
    for (i = 0; i < tabElements.length; i++) {
        if(tabElements[i].id == tabId){
            tabElements[i].hidden = false;
            switches[i].className = "switch active"
        }else{
            tabElements[i].hidden = true;
            switches[i].className = "switch"
        }
    }
}

window.onmessage = function (event) {
    let edata = event.data;
    if (edata.comand) {
        switch (edata.comand) {
            case "setprogram":
                console.log("setprogram");
                let newProgram = JSON.parse(edata.data);
                if (edata.params.forAll) {
                    for (let i = 0; i < WRender.Bots.length; i++) {
                        WRender.Bots[i].programCondition.program = newProgram;
                        WRender.Bots[i].programCondition.Reset();
                    }
                } else {
                    WRender.CurrentBot.programCondition.program = newProgram;
                    WRender.CurrentBot.programCondition.Reset();
                }
                break;
            case "closeprog":
                OpenProgrammator();
                break
            case "stop_prog":
                WRender.CurrentBot.programCondition.Reset();
                WRender.CurrentBot.programCondition.isExecute = false;
                break
            case "start_prog":
                WRender.CurrentBot.programCondition.Reset();
                WRender.CurrentBot.programCondition.isExecute = true;
                break
            default:
                console.warn("неизвестное действие", edata.comand);
                break;
        }
    }
};

const BotstatusPanel = {
    autodigg: document.getElementById("botmode_status_autodigg"),
    roadignore: document.getElementById("botmode_status_roadignore"),
    agr: document.getElementById("botmode_status_agr"),
    debug: document.getElementById("botmode_status_debug"),
    hand: document.getElementById("botmode_status_hand"),
    accuaccelerate: document.getElementById("botmode_status_accuaccelerate"),
    overweight: document.getElementById("botmode_status_overweight"),
    /**
     * @param {BotMode} bmodes 
     */
    SetValues(bmodes) {
        for (const key in bmodes) {
            (this[key].hidden != !bmodes[key]) && (this[key].hidden = !bmodes[key]);
        }
    }
}

class CastomProgressBar {
    /**@type {HTMLDivElement} */
    targetHTMLcontainer;

    #width;
    #height;

    #value = 0;
    set value(val) {
        if (val != this.#value) {
            this.#value = val;
            this.ProgressLineDiv.style.width = `${val > this.#max ? this.#max : val}%`;
            this.PersentText.innerText = `${this.persentsCut? MoneyConverter.toKKKformat(~~this.#value,1000) : ~~this.#value}%`;
        }
    }
    get value() { return this.#LabelDIV.innerText }
    #min = 0;
    #max = 100;
    color;
    bgcolor;
    /**@type {HTMLDivElement} */
    progressBodyDiv;
    /**@type {HTMLDivElement} */
    ProgressLineDiv;
    /**@type {HTMLDivElement} */
    #LabelDIV;
    set Label(val) { 
        if (this.#LabelDIV.innerText != val){
            this.#LabelDIV.innerText = this.LabelDataFormat? this.LabelDataFormat(val) : val;
            this.#LabelDIV.title = this.LabelDataFormat? val : "";
        }
    }
    get Label() { return this.#LabelDIV.innerText }

    /**
     * @param {String} targetHTMLcontainerName 
     * @param {Number} width 
     * @param {Number} height 
     * @param {Number} min 
     * @param {Number} max 
     * @param {Number} value 
     * @param {String} color 
     * @param {String} bgcolor 
     * @param {{LabelDataFormat:Function,persentsCut:Boolean,color:String,bgcolor:String}} params 
     */
    constructor(targetHTMLcontainerName, width, height, value,params) {
        this.targetHTMLcontainer = document.getElementById(targetHTMLcontainerName);

        this.#width = width;
        this.#height = height;
        this.color = params?.color || "dddd";
        this.bgcolor = params?.bgcolor || "#ddd9";
        this.#value = value;
        /**
         * @type {Function(data:any):String}
         */
        this.LabelDataFormat = params?.LabelDataFormat;
        this.persentsCut = params?.persentsCut;

        this.#LabelDIV = document.createElement("div");
        this.#LabelDIV.style.fontSize = `${this.#height}px`;
        this.#LabelDIV.innerText = this.#value;

        this.PersentText = document.createElement("div");
        this.PersentText.className = "castom_progress_persent";
        this.PersentText.style.fontSize = `${this.#height - 1}px`;
        this.PersentText.innerText = `${this.#value}%`;

        this.progressBodyDiv = document.createElement("div");
        this.progressBodyDiv.style.float = "left";
        this.progressBodyDiv.style.width = `${this.#width}px`;
        this.progressBodyDiv.style.height = `${this.#height}px`;
        this.progressBodyDiv.style.backgroundColor = this.bgcolor;

        this.ProgressLineDiv = document.createElement("div");
        this.ProgressLineDiv.style.width = `${this.#value > 100 ? 100 : this.#value}%`;
        this.ProgressLineDiv.style.height = "100%";
        this.ProgressLineDiv.style.backgroundColor = this.color;

        this.targetHTMLcontainer.appendChild(this.#LabelDIV);
        this.targetHTMLcontainer.appendChild(this.progressBodyDiv);

        this.progressBodyDiv.appendChild(this.ProgressLineDiv);
        this.progressBodyDiv.appendChild(this.PersentText);

        //console.log(this.progressBodyDiv, this.ProgressLineDiv, this.targetHTMLcontainer);
    }
}

class BotBasicInfoPanel {
    /**@type {BotBasicInfoPanel} */ static self;

    /**@type {WRenderer} */TargetWRenderer;

    /**@type {HTMLDivElement} */#nickNameDIV;
    /**@type {HTMLDivElement} */#coordsDIV;

    set nickName(val) { if(this.#nickNameDIV.innerText != val)this.#nickNameDIV.innerText = val };
    get nickName() { return this.#nickNameDIV.innerText }

    #botCoordsPrev = new Vector2d(0,0);

    set coords(/** @type {Vector2d}*/ val) { if(!this.#botCoordsPrev.IsEqually(val)) {this.#coordsDIV.innerText = `X:${val.x} Y:${val.y}`; this.#botCoordsPrev.CopyV2(val)}};

    /**
     * @param {WRenderer} targetWRenderer 
     */
    constructor(targetWRenderer) {
        if (BotBasicInfoPanel.self) { return BotBasicInfoPanel.self } else { BotBasicInfoPanel.self = this }
        this.TargetWRenderer = targetWRenderer;
        this.#nickNameDIV = document.getElementById("base_info_nickname");
        this.#coordsDIV = document.getElementById("base_bot_coordinates");
        this.BarHP = new CastomProgressBar("base_info_hp", 200, 15, 15, {color:"#f00"});
        this.BarCryCargoAll = new CastomProgressBar("base_cry_cargo", 50, 15, 0,{persentsCut:true,color: "#ff0",bgcolor:"#aaaa"});
        this.BarCryCargoAll.Label = "Общий груз:";
        /**@type {Map<String,CastomProgressBar>} */
        this.BarCryCargo = new Map([
            ["g", new CastomProgressBar("base_cry_cargo_g", 50, 14, 15,{"LabelDataFormat":(MoneyConverter.toKKKformat),persentsCut:true,color:"#0f0",bgcolor:"#0a08"})],
            ["b", new CastomProgressBar("base_cry_cargo_b", 50, 14, 15,{"LabelDataFormat":(MoneyConverter.toKKKformat),persentsCut:true,color:"#00f",bgcolor:"#00a8"})],
            ["r", new CastomProgressBar("base_cry_cargo_r", 50, 14, 15,{"LabelDataFormat":(MoneyConverter.toKKKformat),persentsCut:true,color:"#f00",bgcolor:"#a008"})],
            ["w", new CastomProgressBar("base_cry_cargo_w", 50, 14, 25,{"LabelDataFormat":(MoneyConverter.toKKKformat),persentsCut:true,color:"#fff",bgcolor:"#aaa8"})],
            ["v", new CastomProgressBar("base_cry_cargo_v", 50, 14, 15,{"LabelDataFormat":(MoneyConverter.toKKKformat),persentsCut:true,color:"#f0f",bgcolor:"#a0a8"})],
            ["c", new CastomProgressBar("base_cry_cargo_c", 50, 14, 66,{"LabelDataFormat":(MoneyConverter.toKKKformat),persentsCut:true,color:"#0ff",bgcolor:"#0aa8"})],
        ])
    }

    Update() {
        if (this.TargetWRenderer?.CurrentBot) {
            let bot = this.TargetWRenderer.CurrentBot;
            this.nickName = bot.nickName;
            this.coords = bot.position;
            this.BarHP.Label = bot.stats.hpNow;
            this.BarHP.value = (bot.stats.hpNow / bot.stats.hpMax) * 100;

            this.BarCryCargoAll.value = bot.cargo.GetVolume() * 100;

            this.BarCryCargo.forEach((barObj, code) => {
                barObj.Label = bot.cargo[code],1000;
                barObj.value = bot.cargo.GetVolume(code) * 100;
            })
        }
    }
}

BotBasicInfoPanel.self = new BotBasicInfoPanel();

//https://habr.com/ru/articles/488516/
//общение с фреймами