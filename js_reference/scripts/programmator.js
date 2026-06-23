const TESTPROGRAM = Object.freeze([{ "type": 0, "id": 0, "data": ["", 0] }]);

const LineLength = 16;

/** 
 * В "скомпилированной" программе второй параметр всегда либо число либо не существует (["nnn","99"]=>["nnn",99])
 * @typedef {[String,Number|undefined]} ProgCellDataArray
 * */

const LogicMode = Object.freeze(
    {
        off: -1,
        none: 0,
        and: 1,
        or: 2,
    }
)

class ProgramStack {
    viewOffset = new Vector2d(0, 0);
    logicalValue = false;
    mode_logic = LogicMode.off;
    returnIndex = -1;

    get reset() {
        this.viewOffset.Set(0, 0);
        this.logicalValue = false;
        this.mode_logic = LogicMode.off;
        this.returnIndex = -1;
    }
}

class VarCacher {
    /**@type {[String,String]} */
    vars = new Array(2);
    YoungerIs = 0;
    /** Предпоследняя записанная переменная */
    get Older() { return this.YoungerIs ? this.vars[1] : this.vars[0] };
    /** Последняя записанная переменная */
    get Younger() { return this.YoungerIs ? this.vars[0] : this.vars[1] };

    Set(/** @type {String} */ variable) {
        if (this.YoungerIs) {
            this.vars[1] = variable
        }
        else {
            this.vars[0] = variable
        }
        this.YoungerIs = (this.YoungerIs + 1) % 2;
    };
    Reset() { [this.vars[0], this.vars[1]] = [null, null] };

    toString(){
        return `[${this.Younger},${this.Older}]`
    }
}

class ReadonlyVariables {
    /**
     * @param {Bot} _botowner 
     */
    constructor(_botowner) {
        this.botOwner = _botowner;
    }
    static ComandList = new Set(["AUT", "AGR", "HND", "DBG", "STK", "DIR", "X", "Y", "CEL", "HP", "HPP", "TIM", "G", "GP", "C", "CP", "R", "RP", "B", "BP", "V", "VP", "W", "WP", "GEO", "GEP", "LOA", "RND", "FLP", "BOO", "IDR", "AX", "AY", "DX", "DY"])
    get AUT() { return this.botOwner.mode.autodigg * 1 };// автокопа ? 1 : 0
    get AGR() { return this.botOwner.mode.agr * 1 };// агрессия ? 1 : 0
    get HND() { return this.botOwner.mode.hand * 1 };// полуручной режим ? 1 : 0
    get DBG() { return this.botOwner.mode.debug * 1 };// дебаг-сообщение [B] ? 1 : 0
    get STK() { return this.botOwner.programCondition.stackDepth };// глубина стэка
    get DIR() { let dir = this.botOwner.rotationIndex; if (dir == 1) { dir = 3 } else if (dir == 3) { dir = 1 } return dir };// направление 0,1,2,3//с переворотом значений с почасового на противочасовой порядок
    get X() { return this.botOwner.position._x };  // координата x
    get Y() { return this.botOwner.position._y };  // координата y
    get CEL() { return this.botOwner.programCondition.viewSelectedBlock };// код клетки
    get HP() { return this.botOwner.stats.hpNow }; // хп
    get HPP() { return ~~(this.botOwner.stats.hpNow / this.botOwner.stats.hpMax) * 100 };// хп%
    get TIM() { return ~~((performance.now() - this.botOwner.programCondition.timeStart) / 1000) };// время в сек. от начала программы
    get G() { return this.botOwner.cargo.g };  // зель
    get GP() { return ~~(this.botOwner.cargo.GetVolume('g') * 100) }; // зель%
    get C() { return this.botOwner.cargo.c };  // голь
    get CP() { return ~~(this.botOwner.cargo.GetVolume('c') * 100) }; // голь%
    get R() { return this.botOwner.cargo.r };  // крась
    get RP() { return ~~(this.botOwner.cargo.GetVolume('r') * 100) }; // крась%
    get B() { return this.botOwner.cargo.b };  // синь
    get BP() { return ~~(this.botOwner.cargo.GetVolume('b') * 100) }; // синь%
    get V() { return this.botOwner.cargo.v };  // фиол
    get VP() { return ~~(this.botOwner.cargo.GetVolume('v') * 100) }; // фиол%
    get W() { return this.botOwner.cargo.w };  // бель
    get WP() { return ~~(this.botOwner.cargo.GetVolume('w') * 100) }; // бель%
    get GEO() { return this.botOwner.cargo.geoFilledValue };// сколько в геологии
    get GEP() { return ~~(this.botOwner.cargo.geoFilledPersent) };// сколько в геологии%
    get LOA() { return ~~(this.botOwner.cargo.GetVolume() * 100) };// груз%
    get RND() { return RandInt(0, 999) };// 0-999
    get FLP() { return this.botOwner.programCondition.mode_flip * 1 };// 
    get BOO() { let logic = this.botOwner.programCondition.currentStack.mode_logic; return logic <= 0 ? 0 : logic } //режим булева оператора (0-REWRITE, 1-AND, 2-OR)
    get IDR() { } //переменные IDR - направление инвентаря (0,1,2,3-как DIR, 5-сброшено)
    get AX() { return Math.abs(this.botOwner.programCondition.currentStack.viewOffset._x) };// расстояние до курсора просмотра клетки по X, Y
    get AY() { return Math.abs(this.botOwner.programCondition.currentStack.viewOffset._y) };// 
    get DX() { return 100 + this.botOwner.programCondition.currentStack.viewOffset._x };// 100 + разница до курсора просмотра клетки по X, Y напр. DY=101 - курсор на клетку ниже, DX=98 - курсор на 2 клетки левее
    get DY() { return 100 + this.botOwner.programCondition.currentStack.viewOffset._y };// 
}

/**
*костыль-переменные - команды SET, ADD, MUL, DIV, SUB, MOD = изменение последней нестандартной переменной,
*AD2, MU2, DI2, SU2 = к предпоследней прибавить/умножить и пр. последнюю переменную 
*/
class PComands {
    static ComandList = new Set(["SET", "ADD", "MUL", "DIV", "SUB", "AD2", "MU2", "DI2", "SU2", "MOD"]);

    static IsValidToOperating1(/** @type {VarCacher}*/lastVars) { return VarType.isWriteble(lastVars.Younger) };
    static IsValidToOperating2(/** @type {VarCacher}*/lastVars) { return VarType.isVariable(lastVars.Younger) && VarType.isWriteble(lastVars.Older) };

    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static SET(condition, comandData) {if (PComands.IsValidToOperating1(condition.lastVariables)) {condition.SetValueToVariable(condition.lastVariables.Younger,comandData[1]);return true;};return false;}
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static ADD(condition, comandData) {if (PComands.IsValidToOperating1(condition.lastVariables)) {condition.SetValueToVariable(condition.lastVariables.Younger, condition.GetValueOfVariable(condition.lastVariables.Younger) + comandData[1]);return true;}return false;}
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static MUL(condition, comandData) {if (PComands.IsValidToOperating1(condition.lastVariables)) {condition.SetValueToVariable(condition.lastVariables.Younger, condition.GetValueOfVariable(condition.lastVariables.Younger) * comandData[1]);return true;}return false;}
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static DIV(condition, comandData) {if (PComands.IsValidToOperating1(condition.lastVariables)) {condition.SetValueToVariable(condition.lastVariables.Younger, ~~(condition.GetValueOfVariable(condition.lastVariables.Younger) / (comandData[1] ? comandData[1]: 1) ));return true;}return false;}
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static SUB(condition, comandData) {if (PComands.IsValidToOperating1(condition.lastVariables)) {condition.SetValueToVariable(condition.lastVariables.Younger, condition.GetValueOfVariable(condition.lastVariables.Younger) - comandData[1]);return true;}return false;}
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static MOD(condition, comandData) {if (PComands.IsValidToOperating1(condition.lastVariables)) {condition.SetValueToVariable(condition.lastVariables.Younger, condition.GetValueOfVariable(condition.lastVariables.Younger) % comandData[1]);return true;}return false;}
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static AD2(condition, comandData) {
        let lastVariables = condition.lastVariables;
        if (PComands.IsValidToOperating2(lastVariables)) {
            let s1 = condition.GetValueOfVariable(lastVariables.Younger);
            let s2 = condition.GetValueOfVariable(lastVariables.Older);
            condition.SetValueToVariable(condition.lastVariables.Older, s2 + s1);
        }
    }
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static MU2(condition, comandData) {
        let lastVariables = condition.lastVariables;
        if (PComands.IsValidToOperating2(lastVariables)) {
            let s1 = condition.GetValueOfVariable(lastVariables.Younger);
            let s2 = condition.GetValueOfVariable(lastVariables.Older);
            condition.SetValueToVariable(condition.lastVariables.Older, s2 * s1);
        }
    }
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static DI2(condition, comandData) {
        let lastVariables = condition.lastVariables;
        if (PComands.IsValidToOperating2(lastVariables)) {
            let s1 = condition.GetValueOfVariable(lastVariables.Younger);
            let s2 = condition.GetValueOfVariable(lastVariables.Older);
            condition.SetValueToVariable(condition.lastVariables.Older, ~~(s2 / (s1? s1: 1)));
        }
    }
    /**@param {ProgramCondition} condition @param {ProgCellDataArray} comandData */
    static SU2(condition, comandData) {
        let lastVariables = condition.lastVariables;
        if (PComands.IsValidToOperating2(lastVariables)) {
            let s1 = condition.GetValueOfVariable(lastVariables.Younger);
            let s2 = condition.GetValueOfVariable(lastVariables.Older);
            condition.SetValueToVariable(lastVariables.Older, s2 - s1);
        }
    }
}

const VarType = {
    isVariable(/**@type {ProgCellDataArray} */data) { return data && (ReadonlyVariables.ComandList.has(data) || (!PComands.ComandList.has(data))) },
    isWriteble(/**@type {ProgCellDataArray} */data) { return data && (!ReadonlyVariables.ComandList.has(data)) && (!PComands.ComandList.has(data)) }
}

class ProgramCondition {
    timeStart = ~~performance.now();

    /**
     * @type {{ "type": Number, "id": Number, "data": ProgCellDataArray}[]}
     */
    program = TESTPROGRAM;

    stackDepthMax = 500;
    startIndex = 0;

    isExecute = false;
    #isHandModeActive = false;
    get isHandModeActive(){return this.#isHandModeActive};
    set isHandModeActive(val){this.botOwner.mode.hand = this.#isHandModeActive = val};

    get isManualControlAlloved(){return this.isExecute ? this.isHandModeActive :  true}

    _currIndex = 0;
    prevIndex = 0;
    get currIndex() { return this._currIndex }
    set currIndex(value) { this.prevIndex = this._currIndex; this._currIndex = value }

    MACROS_rotate_cache = null;

    stackDepth = 0;
    mode_flip = false;

    /**@type {Map<String,Number|String>}*/
    uservariables;

    lastVariables = new VarCacher();

    /** @property {Array<ProgramStack>} stack*/
    stack = [new ProgramStack()]

    get viewX() { return this.botOwner.position.x + this.currentStack.viewOffset.x }
    get viewY() { return this.botOwner.position.y + this.currentStack.viewOffset.y }
    get currentStack() { return this.stack[this.stackDepth] }
    get flipValue() { return this.mode_flip ? -1 : 1 }
    /** Данные с текущей ячейки программы */
    get currentData() { return this.program[this.currIndex].data }
    get viewSelectedBlock() { return this.botOwner.wrenderer.WorldMap.GetIDByCoord(this.viewX, this.viewY) }

    /**
     * @param {Bot} botOwner 
     */
    constructor(botOwner) {
        this.botOwner = botOwner;
        this.readonlyVariables = new ReadonlyVariables(botOwner);
        this.uservariables = new Map();
    }

    /**
     * @param {String} varname 
     * @returns {Number}
     */
    GetValueOfVariable(varname) {
        if (ReadonlyVariables.ComandList.has(varname)) { return this.readonlyVariables[varname] }
        if (this.uservariables.has(varname)) { return this.uservariables.get(varname); }
    }

    /**
     * @param {String} varname 
     * @param {Number} value
     * @returns {Boolean} success
     */
    SetValueToVariable(varname, value) {
        if (this.uservariables.has(varname)) {
            this.uservariables.set(varname, value);
            return true
        }
        return false;
    }

    compareWithLogicValue(value) {
        switch (this.currentStack.mode_logic) {
            case LogicMode.or:
                this.currentStack.logicalValue = this.currentStack.logicalValue || value;
                break;
            case LogicMode.and:
                this.currentStack.logicalValue = this.currentStack.logicalValue && value;
                break;
            case LogicMode.none:
                this.currentStack.logicalValue = value;
                break;
            case LogicMode.off:
                this.currentStack.mode_logic = LogicMode.none;
                this.currentStack.logicalValue = value;
                break;
        }
    }

    GetState() {
        return {
            handMode:this.isHandModeActive,
            nowExecute: this.isExecute,
            prevIndex: this.prevIndex,
            index: this.currIndex,
            stack: this.currentStack,
            stackDepth: this.stackDepth,
            botPosition: this.botOwner.position,
            mode_flip: this.mode_flip,
            lastVariables: this.lastVariables.toString(),
            uservariables: this.uservariables,
            startIndex: this.startIndex,
            Timer: this.isExecute ? ~~performance.now() - this.timeStart : 0,
        }
    }

    Reset() {
        this.stack ? this.stack[0].reset : this.stack = [new ProgramStack()];
        this.uservariables.clear();
        this.stackDepth = 0;
        this.currIndex = 0;
        this.mode_flip = false;
        this.MACROS_rotate_cache = null;
        this.timeStart = ~~performance.now();
        this.isHandModeActive = false;
    }
}

var PROGRAMATOR_DEBUG_LOGGING = false;
let dbgL = (...val) => {
    if (PROGRAMATOR_DEBUG_LOGGING) {
        console.log("c: " + val.toString() + ` t:${performance.now().toFixed(1)}`);
    }
}

class ProgramExecutor {
    bots;
    /**
     * @param {Array<Bot>} bots
     */
    constructor(bots) {
        this.bots = bots;
    }

    Execute() {
        let condition;
        let program;
        let noIterateIndex;
        for (let i = 0; i < this.bots.length; i++) {
            condition = this.bots[i].programCondition;
            if (condition.isExecute) {
                program = condition.program;
                dbgL("index:" + condition.currIndex);
                noIterateIndex = ExecutorList[program[condition.currIndex].id](condition);

                if (!noIterateIndex) {
                    if (condition.currIndex % LineLength == LineLength - 1) {
                        condition.currIndex -= (LineLength - 1);
                    }
                    else {
                        condition.currIndex++;
                    }
                }
                else {
                    noIterateIndex = false;
                }
                //console.clear();
                //console.table(condition.uservariables);
                //if(){}
                //console.log(currentcondition);
            }
        }
    }
}

/** @type {((condition: ProgramCondition) => void|Boolean)[]} */
const ExecutorList = Object.freeze([
    function (condition) {
        let index = condition.currIndex;
        while (condition.program[index].id == 0) {
            if (index % LineLength == LineLength - 1) {
                index = condition.startIndex;
                condition.prevIndex = condition._currIndex = index;
                return true
            }
            else {
                index++;
                if (index >= condition.program.length) {
                    condition.currIndex = condition.startIndex
                    return true
                };
            }

            dbgL("empty");
        }
        condition.prevIndex = condition._currIndex = index;
        return true;
    },//empty
    function (condition) {
        dbgL("newline");
        condition.currIndex = (Math.floor(condition.currIndex / LineLength) + 1) * LineLength;
        return true;
    },
    (condition) => { dbgL("mark"); },//mark: 2,//метка всех функций
    (condition) => {
        dbgL("GO_SUB");
        condition.stackDepth++;
        if (condition.stack[condition.stackDepth] == undefined) { condition.stack.push(new ProgramStack()) } else { condition.stack[condition.stackDepth].reset }
        condition.stack[condition.stackDepth].returnIndex = condition._currIndex;

        let toindex = condition.program[condition.currIndex].data[0];
        toindex = toindex >= 0 ? toindex : condition.startIndex;
        condition.currIndex = toindex;
        return true;
    },//GO_SUB: 3,
    (condition) => { dbgL("look_WA"); condition.currentStack.viewOffset.Set(-condition.flipValue, -1) },//look_WA: 4,
    (condition) => { dbgL("look_W"); condition.currentStack.viewOffset.Set(0, -1) },//look_W: 5,
    (condition) => { dbgL("look_DW"); condition.currentStack.viewOffset.Set(condition.flipValue, -1) },//look_DW: 6,
    (condition) => { dbgL("look_A"); condition.currentStack.viewOffset.Set(-condition.flipValue, 0) },//look_A: 7,
    (condition) => { dbgL("non_use_8"); },//non_use_8: 8,
    (condition) => { dbgL("look_D"); condition.currentStack.viewOffset.Set(condition.flipValue, 0) },//look_D: 9,
    (condition) => { dbgL("look_AS"); condition.currentStack.viewOffset.Set(-condition.flipValue, 1) },//look_AS: 10,
    (condition) => { dbgL("look_S"); condition.currentStack.viewOffset.Set(0, 1) },//look_S: 11,
    (condition) => {
        dbgL("look_l");
        switch (condition.botOwner.rotationIndex) {
            case 0: condition.currentStack.viewOffset.Add(-1, 0); break;
            case 1: condition.currentStack.viewOffset.Add(0, -1); break;
            case 2: condition.currentStack.viewOffset.Add(1, 0); break;
            case 3: condition.currentStack.viewOffset.Add(0, 1); break;
        }
    },//look_l: 12,
    (condition) => {
        dbgL("look_r");
        switch (condition.botOwner.rotationIndex) {
            case 0: condition.currentStack.viewOffset.Add(1, 0); break;
            case 1: condition.currentStack.viewOffset.Add(0, 1); break;
            case 2: condition.currentStack.viewOffset.Add(-1, 0); break;
            case 3: condition.currentStack.viewOffset.Add(0, -1); break;
        }
    },//look_r: 13,
    (condition) => {
        dbgL("start");
        condition.timeStart = ~~performance.now();
        condition.startIndex = condition.currIndex;
    },//start: 14,
    (condition) => {
        dbgL("stop");
        condition.isExecute = false;
        condition.Reset()
    },//stop: 15,
    (condition) => {
        dbgL("GO_STATE");
        condition.stackDepth++;
        if (condition.stack[condition.stackDepth] == undefined) { condition.stack.push(new ProgramStack()) } else { condition.stack[condition.stackDepth].reset }
        let newstack = condition.stack[condition.stackDepth];
        let prevStack = condition.stack[condition.stackDepth - 1];

        newstack.returnIndex = condition._currIndex;
        newstack.logicalValue = prevStack.logicalValue;
        newstack.mode_logic = prevStack.mode_logic;
        newstack.viewOffset.CopyV2(prevStack.viewOffset);

        let toindex = condition.program[condition.currIndex].data[0];
        toindex = toindex >= 0 ? toindex : condition.startIndex;
        condition.currIndex = toindex;
        return true;
    },//GO_STATE: 16,
    (condition) => {
        dbgL("GO_FUNC");
        condition.stackDepth++;
        if (condition.stack[condition.stackDepth] == undefined) { condition.stack.push(new ProgramStack()) } else { condition.stack[condition.stackDepth].reset }
        let newstack = condition.stack[condition.stackDepth];
        let prevStack = condition.stack[condition.stackDepth - 1];

        newstack.returnIndex = condition._currIndex;
        if (prevStack.mode_logic >= 0) {
            newstack.logicalValue = prevStack.logicalValue;
            newstack.mode_logic = LogicMode.none;
        }
        newstack.viewOffset.CopyV2(prevStack.viewOffset);


        let toindex = condition.program[condition.currIndex].data[0];
        toindex = toindex >= 0 ? toindex : condition.startIndex;
        condition.currIndex = toindex;
        return true;
    },//GO_FUNC: 17,
    (condition) => { dbgL("look_SD"); condition.currentStack.viewOffset.Set(condition.flipValue, 1) },//look_SD: 18,
    (condition) => { dbgL("look_w"); condition.currentStack.viewOffset.Add(0, -1) },//look_w: 19,
    (condition) => { dbgL("look_a"); condition.currentStack.viewOffset.Add(-condition.flipValue, 0) },//look_a: 20,
    (condition) => { dbgL("look_s"); condition.currentStack.viewOffset.Add(0, 1) },//look_s: 21,
    (condition) => { dbgL("look_d"); condition.currentStack.viewOffset.Add(condition.flipValue, 0) },//look_d: 22,
    (condition) => {
        dbgL("look_F");
        switch (condition.botOwner.rotationIndex) {
            case 0: condition.currentStack.viewOffset.Set(0, -1); break;
            case 1: condition.currentStack.viewOffset.Set(1, 0); break;
            case 2: condition.currentStack.viewOffset.Set(0, 1); break;
            case 3: condition.currentStack.viewOffset.Set(-1, 0); break;
        }
    },//look_F: 23,
    (condition) => {
        dbgL("look_f");
        switch (condition.botOwner.rotationIndex) {
            case 0: condition.currentStack.viewOffset.Add(0, -1); break;
            case 1: condition.currentStack.viewOffset.Add(1, 0); break;
            case 2: condition.currentStack.viewOffset.Add(0, 1); break;
            case 3: condition.currentStack.viewOffset.Add(-1, 0); break;
        }
    },//look_f: 24,
    (condition) => {
        dbgL("look_L");
        switch (condition.botOwner.rotationIndex) {
            case 0: condition.currentStack.viewOffset.Set(-1, 0); break;
            case 1: condition.currentStack.viewOffset.Set(0, -1); break;
            case 2: condition.currentStack.viewOffset.Set(1, 0); break;
            case 3: condition.currentStack.viewOffset.Set(0, 1); break;
        }
    },//look_L: 25,
    (condition) => {
        dbgL("look_R");
        switch (condition.botOwner.rotationIndex) {
            case 0: condition.currentStack.viewOffset.Set(1, 0); break;
            case 1: condition.currentStack.viewOffset.Set(0, 1); break;
            case 2: condition.currentStack.viewOffset.Set(-1, 0); break;
            case 3: condition.currentStack.viewOffset.Set(0, -1); break;
        }
    },//look_R: 26,
    (condition) => {
        dbgL("27");
        let data = condition.program[condition.currIndex].data[0];
        condition.currIndex = data >= 0 ? data : condition.startIndex;
        condition.currentStack.mode_logic = LogicMode.off;
        condition.currentStack.viewOffset.Set(0, 0);
        return true;
    },//GO_TO: 27,
    (condition) => { dbgL("28"); },//DBG_MSG: 28,
    (condition) => { dbgL("29"); },//DBG_PAUSE: 29,
    (condition) => { 
        dbgL("HAND_MODE_ON");
        condition.isHandModeActive = true;
    },//HAND_MODE_ON: 30,
    (condition) => {
        condition.isHandModeActive = false;
        dbgL("HAND_MODE_OFF"); 
    },//HAND_MODE_OFF: 31,
    (condition) => { dbgL("32"); },//IF_RESP: 32,
    (condition) => {
        dbgL("IF_TRUE", condition.currentStack.logicalValue, condition.currentStack.mode_logic);
        let toindex = condition.program[condition.currIndex].data[0];
        toindex = toindex >= 0 ? toindex : condition.startIndex;

        if (!condition.currentStack.logicalValue || condition.currentStack.mode_logic == LogicMode.off) {
            condition.currIndex = toindex;
            condition.currentStack.mode_logic = LogicMode.off;
            return true;
        }
        condition.currentStack.mode_logic = LogicMode.off;
    },//IF_TRUE: 33,
    (condition) => {
        dbgL("IF_FALSE");
        let toindex = condition.program[condition.currIndex].data[0];
        toindex = toindex >= 0 ? toindex : condition.startIndex;

        //console.log(condition.currentStack.logicalValue,condition.currentStack.mode_logic==LogicMode.off);
        if (condition.currentStack.logicalValue && condition.currentStack.mode_logic != LogicMode.off) {
            condition.currIndex = toindex;
            condition.currentStack.mode_logic = LogicMode.off;
            return true;
        }
        condition.currentStack.mode_logic = LogicMode.off;
    },//IF_FALSE: 34,

    (condition) => { dbgL("35"); },//MACROS_GUN: 35,
    (condition) => {
        dbgL("MACROS_DIGG");
        if (!condition.botOwner.inCooldown) {
            let block = condition.botOwner.BlockAhead;
            if (BlockStats[block].solid && BlockStats[block].hardness >= 0) {
                condition.botOwner.Digg()
                return true
            }
        }
        else {
            return true
        }
        condition.botOwner.cooldown = BotCDConstants.macrosCD;
    },//MACROS_DIGG: 36,
    (condition) => {
        dbgL("MACROS_BLOCK");
        if (!condition.botOwner.inCooldown) {
            let block = condition.botOwner.BlockAhead;
            let stats = BlockStats[block];
            if (stats.hardness >= 0 || stats.replesable) {
                if (stats.replesable || block == Block.block_green || block == Block.block_yellow) {
                    condition.botOwner.SetBlock();
                    if (condition.botOwner.BlockAhead == Block.block_red) {
                        condition.botOwner.cooldown = BotCDConstants.macrosCD;
                        return false
                    }
                    else {
                        return true
                    }
                }
                else {
                    condition.botOwner.Digg();
                    return true
                }
            }
            condition.botOwner.cooldown = BotCDConstants.macrosCD;
            return false;
        }
        return true
    },//MACROS_BLOCK: 37,
    (condition) => {
        dbgL("MACROS_HEAL");
        if (!condition.botOwner.inCooldown) {
            if (condition.botOwner.stats.hpNow < condition.botOwner.stats.hpMax) {
                condition.botOwner.Heal();
                return true;
            }
            condition.botOwner.cooldown = BotCDConstants.macrosCD;
            return false;
        }
        return true;
    },//MACROS_HEAL: 38,
    (condition) => {
        dbgL("MACROS_DIGG_AROUND");
        let botowner = condition.botOwner;
        if (!botowner.inCooldown) {
            if (condition.MACROS_rotate_cache == null) { condition.MACROS_rotate_cache = botowner.rotationIndex }

            botowner.rotationIndex = condition.MACROS_rotate_cache;

            if (BlockStats[botowner.BlockLeft].is_cry) {
                botowner.rotationIndex += 3;
                botowner.Digg()
                return true
            }
            else if (BlockStats[botowner.BlockRight].is_cry) {
                botowner.rotationIndex++;
                botowner.Digg()
                return true
            }
            else if (BlockStats[botowner.BlockAhead].is_cry) {
                botowner.Digg()
                return true
            }
            condition.MACROS_rotate_cache = null;
            botowner.cooldown = BotCDConstants.macrosCD;
            return false;
        }
        return true
    },//MACROS_DIGG_AROUND: 39,
    (condition) => {
        dbgL("OR");
        condition.currentStack.mode_logic = LogicMode.or;
    },//OR: 40,
    (condition) => {
        dbgL("AND");
        condition.currentStack.mode_logic = LogicMode.and;
    },//AND: 41,
    (condition) => { dbgL("FLIP"); condition.mode_flip = !condition.mode_flip },//FLIP: 42,
    (condition) => { dbgL("AUTODIGG_ON"); condition.botOwner.mode.autodigg = true; },//AUTODIGG_ON: 43,
    (condition) => { dbgL("AUTODIGG_OF"); condition.botOwner.mode.autodigg = false; },//AUTODIGG_OF: 44,
    (condition) => {
        dbgL("RETURN");
        if (condition.stackDepth > 0) {
            condition.currIndex = condition.currentStack.returnIndex;
            condition.stackDepth--;
            condition.currentStack.mode_logic = LogicMode.off;
            condition.currentStack.viewOffset.Set(0, 0);
        }
        else {
            console.error("Встречен возврат без входа в подфункцию");
        }
    },//RETURN: 45,
    (condition) => {
        dbgL("RETURN_FUNK");
        if (condition.stackDepth > 0) {
            condition.currIndex = condition.currentStack.returnIndex;
            let oldStack = condition.currentStack;

            condition.stackDepth--;
            if (oldStack.mode_logic != LogicMode.off) {
                condition.currentStack.mode_logic = LogicMode.none;
                condition.currentStack.logicalValue = oldStack.logicalValue;
            }
        }
        else {
            console.error("Встречен возврат без входа в подфункцию");
        }

    },//RETURN_FUNK: 46,
    (condition) => {
        dbgL("RETURN_STATE");
        if (condition.stackDepth > 0) {
            condition.currIndex = condition.currentStack.returnIndex;
            let oldStack = condition.currentStack;
            condition.stackDepth--;

            condition.currentStack.mode_logic = oldStack.mode_logic;
            condition.currentStack.logicalValue = oldStack.logicalValue;
            condition.currentStack.viewOffset.CopyV2(oldStack.viewOffset);
        }
        else {
            console.error("Встречен возврат без входа в подфункцию");
        }

    },//RETURN_STATE: 47,
    (condition) => {
        dbgL("VAR_EQUAL");
        let data = condition.currentData;
        if (data[0]) {
            let result = condition.readonlyVariables[data[0]];
            if (result != null) {
                condition.compareWithLogicValue(result == data[1]);
                condition.lastVariables.Set(data[0]);
            }
            else if (PComands.ComandList.has(data[0])) {
                PComands[data[0]](condition, data);
            }
            else {
                result = condition.uservariables.get(data[0]);
                if (result != null) {
                    condition.compareWithLogicValue((result == data[1]));
                }
                else {
                    condition.uservariables.set(data[0], data[1]);
                }
                condition.lastVariables.Set(data[0]);
            }
            //console.log(data,result,result == data[1]);
        }
    },//VAR_EQUAL: 48,
    (condition) => {
        dbgL("VAR_MORE");
        let data = condition.currentData;
        if (data[0]) {
            let result = condition.readonlyVariables[data[0]];
            if (result != null) {
                condition.compareWithLogicValue(result > data[1]);
            }
            else {
                result = condition.uservariables.get(data[0]);
                condition.compareWithLogicValue((result != null) && (result > data[1]));
            }
            condition.lastVariables.Set(data[0]);
            //console.log(data,result,result > data[1]);
        }
    },//VAR_MORE: 49,
    (condition) => {
        dbgL("VAR_LESS");
        let data = condition.currentData;
        if (data[0]) {
            let result = condition.readonlyVariables[data[0]];
            if (result != null) {
                condition.compareWithLogicValue(result < data[1]);
            }
            else {
                result = condition.uservariables.get(data[0]);
                condition.compareWithLogicValue((result != null) && (result < data[1]));
            }
            condition.lastVariables.Set(data[0]);
            //console.log(data,result,result < data[1]);
        }
    },//VAR_LESS: 50,

    (condition) => { dbgL("51"); },//ONLINE_BOOM: 51,
    (condition) => { dbgL("52"); },//ONLINE_RAZ: 52,
    (condition) => { dbgL("53"); },//ONLINE_PROT: 53,
    (condition) => { dbgL("54"); },//ONLINE_GEO: 54,
    (condition) => { dbgL("55"); },//ONLINE_ZZ: 55,
    (condition) => { dbgL("56"); },//ONLINE_C190: 56,
    (condition) => { dbgL("57"); },//ONLINE_POLY: 57,
    (condition) => { dbgL("58"); },//ONLINE_UP: 58,
    (condition) => { dbgL("59"); },//ONLINE_CRAFT: 59,
    (condition) => { dbgL("60"); },//ONLINE_NANO: 60,
    (condition) => { dbgL("61"); },//ONLINE_REM: 61,
    (condition) => {
        dbgL("m_u");
        if (condition.botOwner.inCooldown) { return true } condition.botOwner.Move('u'); condition.currentStack.viewOffset.Set(0, 0)
    },//move_W: 62,
    (condition) => {
        dbgL("m_l");
        if (condition.botOwner.inCooldown) { return true }
        if (condition.mode_flip) { condition.botOwner.Move('r') }
        else { condition.botOwner.Move('l') }; condition.currentStack.viewOffset.Set(0, 0)
    },//move_A: 63,
    (condition) => {
        dbgL("m_d");
        if (condition.botOwner.inCooldown) { return true } condition.botOwner.Move('d'); condition.currentStack.viewOffset.Set(0, 0)
    },//move_S: 64,
    (condition) => {
        dbgL("m_r");
        if (condition.botOwner.inCooldown) { return true }
        if (condition.mode_flip) { condition.botOwner.Move('l') }
        else { condition.botOwner.Move('r') }; condition.currentStack.viewOffset.Set(0, 0)
    },//move_D: 65,
    (condition) => {
        dbgL("digg");
        if (condition.botOwner.inCooldown) { return true } condition.botOwner.Digg()
    },//digg: 66,
    (condition) => {
        dbgL("r_u");
        if (condition.botOwner.inCooldown) { return true }
        condition.botOwner.Rotate('u');
        condition.currentStack.viewOffset.Set(0, 0)
    },//rotate_w: 67,
    (condition) => {
        dbgL("r_l");
        if (condition.botOwner.inCooldown) { return true }
        if (condition.mode_flip) { condition.botOwner.Rotate('r') }
        else { condition.botOwner.Rotate('l') }; condition.currentStack.viewOffset.Set(0, 0)
    },//rotate_a: 68,
    (condition) => {
        dbgL("r_d");
        if (condition.botOwner.inCooldown) { return true } condition.botOwner.Rotate('d');
        condition.currentStack.viewOffset.Set(0, 0)
    },//rotate_s: 69,
    (condition) => {
        dbgL("r_r");
        if (condition.botOwner.inCooldown) { return true }
        if (condition.mode_flip) { condition.botOwner.Rotate('l') }
        else { condition.botOwner.Rotate('r') }; condition.currentStack.viewOffset.Set(0, 0)
    },//rotate_d: 70,
    (condition) => {
        dbgL("71");
    },//non_use_71: 71,
    (condition) => {
        dbgL("move_F");
        if (condition.botOwner.inCooldown) { return true } condition.botOwner.Move('f'); condition.currentStack.viewOffset.Set(0, 0);
    },//move_F: 72,
    (condition) => {
        dbgL("CCW");
        if (condition.botOwner.inCooldown) { return true }
        if (condition.mode_flip) { condition.botOwner.Rotate((condition.botOwner.rotationIndex + 1) % 4) } else { condition.botOwner.Rotate((condition.botOwner.rotationIndex + 3) % 4) };
    },//rotate_CCW: 73,
    (condition) => {
        dbgL("CW");
        if (condition.botOwner.inCooldown) { return true }
        if (condition.mode_flip) { condition.botOwner.Rotate((condition.botOwner.rotationIndex + 3) % 4) } else { condition.botOwner.Rotate((condition.botOwner.rotationIndex + 1) % 4) };
    },//rotate_CW: 74,
    (condition) => { dbgL("set_block"); if (condition.botOwner.inCooldown) { return true } condition.botOwner.SetBlock() },//set_block: 75,
    (condition) => { dbgL("set_geo"); if (condition.botOwner.inCooldown) { return true } condition.botOwner.UseGeo() },//set_geo: 76,
    (condition) => { dbgL("77"); },//beep: 77,
    (condition) => { dbgL("set_WB"); if (condition.botOwner.inCooldown) { return true } condition.botOwner.SetWB() },//set_WB: 78,
    (condition) => { dbgL("set_road"); if (condition.botOwner.inCooldown) { return true } condition.botOwner.SetRoad() },//set_road: 79,
    (condition) => { dbgL("heal"); if (condition.botOwner.inCooldown) { return true } condition.botOwner.Heal() },//heal: 80,
    (condition) => { dbgL("set_quadro"); if (condition.botOwner.inCooldown) { return true } condition.botOwner.SetQuadro() },//set_quadro: 81,
    (condition) => { dbgL("rotate_random"); if (condition.botOwner.inCooldown) { return true } condition.botOwner.Rotate(RandInt(0, 3)) },//rotate_random: 82,
    (condition) => {
        dbgL("INVENTORY_w");

    },//INVENTORY_w: 83,
    (condition) => {
        dbgL("INVENTORY_a");

    },//INVENTORY_a: 84,
    (condition) => {
        dbgL("INVENTORY_s");

    },//INVENTORY_s: 85,
    (condition) => {
        dbgL("INVENTORY_d");

    },//INVENTORY_d: 86,
    (condition) => {
        dbgL("is_no_empty");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].solid);
    },//is_no_empty: 87,
    (condition) => {
        dbgL("is_empty");
        condition.compareWithLogicValue(!BlockStats[condition.viewSelectedBlock].solid);
    },//is_empty: 88,
    (condition) => {
        dbgL("is_falls");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].falltype != null);
    },//is_falls: 89,
    (condition) => {
        dbgL("is_crys");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].is_cry);
    },//is_crys: 90,
    (condition) => {
        dbgL("is_alive");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].is_alive);
    },//is_alive: 91,
    (condition) => {
        dbgL("is_bolder");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].falltype == FallType.bolder);
    },//is_bolder: 92,
    (condition) => {
        dbgL("is_sand");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].falltype == FallType.sand);
    },//is_sand: 93,
    (condition) => {
        dbgL("is_diggable_rock");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].diggablerock);
    },//is_diggable: 94,
    (condition) => {
        dbgL("is_non_diggable");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].solid && BlockStats[condition.viewSelectedBlock].hardness < 0);
    },//is_non_diggable: 95,
    (condition) => {
        dbgL("is_red_rock");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.rock_red);
    },//is_red_rock: 96,
    (condition) => {
        dbgL("is_black_rock");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.rock_black);
    },//is_black_rock: 97,
    (condition) => {/////////////////////////////////////////////////////////////////////////////////////
        dbgL("is_slime");
        condition.compareWithLogicValue(BlockStats[condition.viewSelectedBlock].is_slime);
    },//is_slime: 98,
    (condition) => {
        dbgL("is_box");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.box);
    },//is_box: 99,
    (condition) => {
        dbgL("is_quadro");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.block_quadro);
    },//is_quadro: 100,
    (condition) => {
        dbgL("is_road");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.road);
    },//is_road: 101,
    (condition) => {
        dbgL("is_red_block");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.block_red);
    },//is_red_block: 102,
    (condition) => {
        dbgL("is_yellow_block");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.block_yellow);
    },//is_yellow_block: 103,
    (condition) => {
        dbgL("is_green_block");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.block_green);
    },//is_green_block: 104,
    (condition) => {
        dbgL("is_support_block");
        condition.compareWithLogicValue(condition.viewSelectedBlock == Block.block_support);
    },//is_support_block: 105,
    (condition) => {
        dbgL("is_in_gun"); console.warn("НЕ ЗАБЫТЬ РЕАЛИЗОВАТЬ is_in_gun");
        condition.compareWithLogicValue(false);
    },//is_in_gun: 106,
    (condition) => {
        dbgL("AGR_ON");
        condition.botOwner.mode.agr = true;
    },//AGR_ON: 107,
    (condition) => {
        dbgL("AGR_OFF");
        condition.botOwner.mode.agr = false;
    },//AGR_OFF: 108,
    (condition) => {
        dbgL("HP_half");
        condition.compareWithLogicValue(condition.botOwner.stats.hpNow < (condition.botOwner.stats.hpMax / 2));
    },//HP_half: 109,
    (condition) => {
        dbgL("HP_less100");
        condition.compareWithLogicValue(condition.botOwner.stats.hpNow < condition.botOwner.stats.hpMax);
    },//HP_less100: 110,
])





const CellID = Object.freeze({
    length: 111,
    get_random() { return Math.floor(Math.random() * this.length) },
    error: -1,
    empty: 0,
    new_line: 1,
    mark: 2,//метка всех функций
    GO_SUB: 3,
    look_WA: 4,
    look_W: 5,
    look_DW: 6,
    look_A: 7,
    non_use_8: 8,
    look_D: 9,
    look_AS: 10,
    look_S: 11,
    look_l: 12,
    look_r: 13,
    start: 14,
    stop: 15,
    GO_STATE: 16,
    GO_FUNC: 17,
    look_SD: 18,
    look_w: 19,
    look_a: 20,
    look_s: 21,
    look_d: 22,
    look_F: 23,
    look_f: 24,
    look_L: 25,
    look_R: 26,
    GO_TO: 27,
    DBG_MSG: 28,
    DBG_PAUSE: 29,
    HAND_MODE_ON: 30,
    HAND_MODE_OFF: 31,
    IF_RESP: 32,
    IF_TRUE: 33,
    IF_FALSE: 34,
    MACROS_GUN: 35,
    MACROS_DIGG: 36,
    MACROS_BLOCK: 37,
    MACROS_HEAL: 38,
    MACROS_DIGG_AROUND: 39,
    OR: 40,
    AND: 41,
    FLIP: 42,
    AUTODIGG_ON: 43,
    AUTODIGG_OF: 44,
    RETURN: 45,
    RETURN_FUNK: 46,
    RETURN_STATE: 47,
    VAR_EQUAL: 48,
    VAR_MORE: 49,
    VAR_LESS: 50,
    ONLINE_BOOM: 51,
    ONLINE_RAZ: 52,
    ONLINE_PROT: 53,
    ONLINE_GEO: 54,
    ONLINE_ZZ: 55,
    ONLINE_C190: 56,
    ONLINE_POLY: 57,
    ONLINE_UP: 58,
    ONLINE_CRAFT: 59,
    ONLINE_NANO: 60,
    ONLINE_REM: 61,
    move_W: 62,
    move_A: 63,
    move_S: 64,
    move_D: 65,
    digg: 66,
    rotate_w: 67,
    rotate_a: 68,
    rotate_s: 69,
    rotate_d: 70,
    non_use_71: 71,
    move_F: 72,
    rotate_CCW: 73,
    rotate_CW: 74,
    set_block: 75,
    set_geo: 76,
    beep: 77,
    set_VB: 78,
    set_road: 79,
    heal: 80,
    set_quadro: 81,
    rotate_random: 82,
    INVENTORY_w: 83,
    INVENTORY_a: 84,
    INVENTORY_s: 85,
    INVENTORY_d: 86,
    is_no_empty: 87,
    is_empty: 88,
    is_falls: 89,
    is_crys: 90,
    is_alive: 91,
    is_bolder: 92,
    is_sand: 93,
    is_diggable: 94,
    is_non_diggable: 95,
    is_red_rock: 96,
    is_black_rock: 97,
    is_slime: 98,
    is_box: 99,
    is_quadro: 100,
    is_road: 101,
    is_red_block: 102,
    is_yellow_block: 103,
    is_green_block: 104,
    is_support_block: 105,
    is_in_gun: 106,
    AGR_ON: 107,
    AGR_OFF: 108,
    HP_half: 109,
    HP_less100: 110,
})

const IsOperationFast = Object.freeze([
    true,//empty: 0,
    true,//new_line: 1,
    true,//mark: 2,//метка всех функций
    true,//GO_SUB: 3,
    true,//look_WA: 4,
    true,//look_W: 5,
    true,//look_DW: 6,
    true,//look_A: 7,
    true,//non_use_8: 8,
    true,//look_D: 9,
    true,//look_AS: 10,
    true,//look_S: 11,
    true,//look_l: 12,
    true,//look_r: 13,
    true,//start: 14,
    true,//stop: 15,
    true,//GO_STATE: 16,
    true,//GO_FUNC: 17,
    true,//look_SD: 18,
    true,//look_w: 19,
    true,//look_a: 20,
    true,//look_s: 21,
    true,//look_d: 22,
    true,//look_F: 23,
    true,//look_f: 24,
    true,//look_L: 25,
    true,//look_R: 26,
    true,//GO_TO: 27,
    false,//DBG_MSG: 28,
    false,//DBG_PAUSE: 29,
    true,//HAND_MODE_ON: 30,
    true,//HAND_MODE_OFF: 31,
    true,//IF_RESP: 32,
    true,//IF_TRUE: 33,
    true,//IF_FALSE: 34,
    false,//MACROS_GUN: 35,
    false,//MACROS_DIGG: 36,
    false,//MACROS_BLOCK: 37,
    false,//MACROS_HEAL: 38,
    false,//MACROS_DIGG_AROUND: 39,
    true,//OR: 40,
    true,//AND: 41,
    true,//FLIP: 42,
    true,//AUTODIGG_ON: 43,
    true,//AUTODIGG_OF: 44,
    true,//RETURN: 45,
    true,//RETURN_FUNK: 46,
    true,//RETURN_STATE: 47,
    true,//VAR_EQUAL: 48,
    true,//VAR_MORE: 49,
    true,//VAR_LESS: 50,
    false,//ONLINE_BOOM: 51,
    false,//ONLINE_RAZ: 52,
    false,//ONLINE_PROT: 53,
    false,//ONLINE_GEO: 54,
    false,//ONLINE_ZZ: 55,
    false,//ONLINE_C190: 56,
    false,//ONLINE_POLY: 57,
    false,//ONLINE_UP: 58,
    false,//ONLINE_CRAFT: 59,
    false,//ONLINE_NANO: 60,
    false,//ONLINE_REM: 61,
    false,//move_W: 62,
    false,//move_A: 63,
    false,//move_S: 64,
    false,//move_D: 65,
    false,//digg: 66,
    false,//rotate_w: 67,
    false,//rotate_a: 68,
    false,//rotate_s: 69,
    false,//rotate_d: 70,
    true,//non_use_71: 71,
    false,//move_F: 72,
    false,//rotate_CCW: 73,
    false,//rotate_CW: 74,
    false,//set_block: 75,
    false,//set_geo: 76,
    false,//beep: 77,
    false,//set_VB: 78,
    false,//set_road: 79,
    false,//heal: 80,
    false,//set_quadro: 81,
    false,//rotate_random: 82,
    false,//INVENTORY_w: 83,
    false,//INVENTORY_a: 84,
    false,//INVENTORY_s: 85,
    false,//INVENTORY_d: 86,
    true,//is_no_empty: 87,
    true,//is_empty: 88,
    true,//is_falls: 89,
    true,//is_crys: 90,
    true,//is_alive: 91,
    true,//is_bolder: 92,
    true,//is_sand: 93,
    true,//is_diggable: 94,
    true,//is_non_diggable: 95,
    true,//is_red_rock: 96,
    true,//is_black_rock: 97,
    true,//is_slime: 98,
    true,//is_box: 99,
    true,//is_quadro: 100,
    true,//is_road: 101,
    true,//is_red_block: 102,
    true,//is_yellow_block: 103,
    true,//is_green_block: 104,
    true,//is_support_block: 105,
    false,//is_in_gun: 106,
    true,//AGR_ON: 107,
    true,//AGR_OFF: 108,
    true,//HP_half: 109,
    true//HP_less100: 110,
])