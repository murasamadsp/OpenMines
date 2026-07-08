
const BotIDCounter = { last: 0 }
const CryCodes = Object.freeze(["g", "b", "r", "w", "v", "c"]);
const BaseBotStats = {
    digg: { param: 10, text: "Копание" },
    moveSpeed: { param: 11, text: "Передвижение" },
    moveRoadSpeedMul: { param: 2.5, text: "Передвижение по дорогам" },
    mining: { param: 5, text: "Добыча" },
    miningCrys: {
        param: {
            g: { param: 0, text: "Зеленые" },
            b: { param: 0, text: "Синие" },
            r: { param: 0, text: "Красные" },
            w: { param: 0, text: "Белые" },
            v: { param: 0, text: "Фиолетовые" },
            c: { param: 0, text: "Голубые" },
        },
        text: "Добыча кристаллов",
    },

}

/**
 * кулдауны в миллисекундах
 */
const BotCDConstants = Object.freeze({
    digging: 333,
    rotate: 100,
    heal: 200,
    setblock: 200,
    geo: 200,
    macrosCD: 50,
})

const BOTSKILLSPRESET = [{ id: 0, lvl: 1 }, { id: 1, lvl: 1 }, , { id: 2, lvl: 1 }, { id: 6, lvl: 1 }, , { id: 9, lvl: 1 }, { id: 10, lvl: 1 }, { id: 44, lvl: 1 }, , { id: 22, lvl: 1 }, { id: 24, lvl: 1 }, { id: 51, lvl: 1 }, , { id: 32, lvl: 1 }, { id: 40, lvl: 1 }];

/** @typedef {{id:Number,lvl:Number,xp:Number,isLocked:Boolean}} BotSkillParams */
class BotSkill {
    id; lvl; xp; isLocked;
    constructor(/** @type {BotSkillParams}*/ params) {
        this.id = params?.id || 0;
        this.lvl = params?.lvl || 1;
        this.xp = params?.xp || 0;
        this.isLocked = params?.isLocked || false;
    }
}

class BotSkillsContainer {
    static MaxSkillsCount = 32;

    /**@type {BotSkill[]} */
    list = new Array(BotSkillsContainer.MaxSkillsCount);
    constructor(/** @type {BotSkill[]} */ skills = BOTSKILLSPRESET) {
        for (let i = 0; i < this.list.length; i++) {
            this.list[i] = new BotSkill(skills[i]);
        }
    }
}

//Основные (в том числе вычисленные на основе скиллов) параметры робота
class BotSkillEffects {
    constructor(/** @type {Bot} */ botowner) {
        this.moveSpeed = 20 + Math.random() * 20;
    }

    hpNow = 500;
    hpMax = 500;

    digg = 10;
    geology = 10;

    diggSandOneshotChance = 0;
    diggRocksOneshotChance = 0;
    diggBlocksOneshotChance = 0;
    diggQuadroBlocksOneshotChance = 0;
    /**Деактивация */
    diggSlimeOneshotChance = 0;

    moveSpeed = 20;
    moveRoadSpeedMul = 3.4;

    //глубина охлада
    coolingToDepth = 100;

    //Добыча кристаллов
    mining = 5;
    sorting = 3;
    //промывка
    washing = 0;
    //Смежка
    adjacentMining = 0;

    //Цены на постройку и прочка
    building = {
        green: { cost: 0, hardness: 0 },
        yellow: { cost: 0, hardness: 0 },
        red: { cost: 0, hardness: 0 },
        support: { cost: 0, hardness: 0 },
        quadro: { cost: 0, hardness: 0 },
        war: { cost: 0, hardness: 0 }
    }

    miningCrys = {
        g: 6,
        b: 5,
        r: 4,
        w: 3,
        v: 2,
        c: 1,
    }
    cargo = {
        base: 100,
        g: 10,
        b: 5,
        r: 6,
        w: 2,
        v: 7,
        c: 2,
    }

    visibleToRadar = true;
    overweightdamageMul = 1;
    gunConsumeMul = 1;

    /**
    * keys: g,b,r,w,v,c
    * @param {string} key 
    */
    CryCargoSyze(key) {
        return this.cargo.base * this.cargo[key];
    }
}

class BotMode {
    autodigg = true;
    roadignore = false;
    agr = false;
    debug = false;
    hand = false;
    accuaccelerate = 0;
    overweight = false;
}

class Bot {
    visibleposition = new Vector2d(0, 0);
    /** @type {(bot:Bot) : void>} */
    _onBotMoved = () => { };
    get onBotMoved() { return this._onBotMoved; }
    set onBotMoved(cb) { if (typeof (cb) == "function") this._onBotMoved = cb; else this._onBotMoved = () => { }; }

    /** @type {(bot:Bot) : void>} */
    _onRespawned = () => { };
    get onRespawned() { return this._onRespawned; }
    set onRespawned(cb) { if (typeof (cb) == "function") this._onRespawned = cb; else this._onRespawned = () => { }; }

    /**
     * @param {Vector2d} pos 
     * @param {number} skinID
     * @param {String} [nickName=NickNames.unique]
     * @param {WRenderer} wrenderer 
     * @param {Tails} tail 
     * @param {BotSkillEffects} botstats
     * @param {ProgramCondition} programCondition
     * @param {BotSkillsContainer} [botOwnSkills=new BotSkillsContainer()] 
     */
    constructor(pos = new Vector2d(0, 0), skinID = 0, nickName = NickNames.unique, wrenderer, tail = new Tails(this), botstats = new BotSkillEffects(this),botOwnSkills = new BotSkillsContainer()) {
        this.visibleposition.CopyV2(pos);
        this.cachedPosition = new Vector2d(pos)
        this.position = pos;
        this.visiblerotation = 0;
        this.rotation = 0;
        this.skin = skinID;
        this.tail = tail;

        this.nickName = nickName ?? NickNames.unique;
        BotIDCounter.last = this.id = BotIDCounter.last + 1;
        this.clanID = null;

        this.stats = botstats;
        this.OwnSkills = botOwnSkills;
        this.cargo = new BotCargo(botstats);
        this.inventory = new BotItemInventory();

        this.inventory.GenerateRandomItems(20, 500000);

        this.mode = new BotMode();

        this.wrenderer = wrenderer;

        this.isVisible = false;

        this.programCondition = new ProgramCondition(this);
    }

    get rotationIndex() { return Math.floor(this.rotation / 90) % 4 };
    set rotationIndex(value) { this.rotation = (value % 4) * 90 };
    _cooldown = 0;
    set cooldown(v) { this._cooldown = v; this._cdTime = performance.now(); } get cooldown() { return this._cooldown };
    _cdTime = 0;
    get cdTime() { return this._cdTime };
    get inCooldown() { return (this.cdTime + this.cooldown) > performance.now() }


    get bFrontX() { return this.position.x + (this.rotation == 90 ? 1 : this.rotation == 270 ? -1 : 0) };
    get bFrontY() { return this.position.y + (this.rotation == 0 ? -1 : this.rotation == 180 ? 1 : 0) };

    get BlockAhead() { return this.wrenderer.WorldMap.GetIDByCoord(this.bFrontX, this.bFrontY); }

    get BlockLeft() {
        switch (this.rotationIndex) {
            case 0: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x - 1, this.position.y);
            case 1: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x, this.position.y - 1);
            case 2: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x + 1, this.position.y);
            case 3: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x, this.position.y + 1);
        }
    }

    get BlockRight() {
        switch (this.rotationIndex) {
            case 0: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x + 1, this.position.y);
            case 1: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x, this.position.y + 1);
            case 2: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x - 1, this.position.y);
            case 3: return this.wrenderer.WorldMap.GetIDByCoord(this.position.x, this.position.y - 1);
        }
    }

    cachedBlockUnder = 0;

    CheckBlockUnder() {
        let b = this.wrenderer.WorldMap.GetIDByCoord(this.position.x, this.position.y);
        if (b != this.cachedBlockUnder) {
            if (BlockStats[b].solid) {
                this.wrenderer.WorldMap.ClearTile(this.position.x, this.position.y);
                if (this.isVisible) this.wrenderer.finiteAnims.add(Anim.GetExemplar(AnimID.geo, this.position, this.rotation));
            }
        }
    }

    Teleport(x, y) {
        this.position.x = x;
        this.position.y = y;
        this.HasMoved();
    }

    HasMoved() {
        if (!this.cachedPosition.IsEqually(this.position)) {
            this.cachedPosition.CopyV2(this.position);
            this._onBotMoved(this);
        }
    }

    Respawn() {

    }

    /**TODO страшный говнокод */
    Move(side = "f", fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            let moved = false;
            let cd = 0;

            if (!this.inCooldown) {
                let map = this.wrenderer.WorldMap;
                let underBot = map.GetIDByCoord(this.position.x, this.position.y);
                if ((!this.mode.roadignore || !this.programCondition.isManualControlAlloved) && (underBot == Block.road || underBot == Block.g_road)) {
                    cd = 1000 / (this.stats.moveSpeed * this.stats.moveRoadSpeedMul);
                }
                else {
                    cd = 1000 / this.stats.moveSpeed;
                }
                //console.log(cd.toFixed(1),`[${(1000/cd).toFixed(1)} m/s]`,delta.toFixed(1), this.cooldown);
                switch (side) {
                    case "u":
                    case 0:
                        this.rotation = 0;
                        if (!BlockStats[map.GetIDByCoord(this.position.x, this.position.y - 1)].solid || WOptions.ignoreBorders) {
                            this.Trails();
                            this.position.y--;
                            moved = true;
                        }
                        else if (this.mode.autodigg) {
                            this.Digg();
                        }
                        break;
                    case "d":
                    case 2:
                        this.rotation = 180;
                        if (!BlockStats[map.GetIDByCoord(this.position.x, this.position.y + 1)].solid || WOptions.ignoreBorders) {
                            this.Trails();
                            this.position.y++;
                            moved = true;
                        }
                        else if (this.mode.autodigg) {
                            this.Digg();
                        }
                        break;
                    case "l":
                    case 3:
                        this.rotation = 270;
                        if (!BlockStats[map.GetIDByCoord(this.position.x - 1, this.position.y)].solid || WOptions.ignoreBorders) {
                            this.Trails();
                            this.position.x--;
                            moved = true;
                        }
                        else if (this.mode.autodigg) {
                            this.Digg();
                        }
                        break;
                    case "r":
                    case 1:
                        this.rotation = 90;
                        if (!BlockStats[map.GetIDByCoord(this.position.x + 1, this.position.y)].solid || WOptions.ignoreBorders) {
                            this.Trails();
                            this.position.x++;
                            moved = true;
                        }
                        else if (this.mode.autodigg) {
                            this.Digg();
                        }
                        break;
                    case "f":
                    case 4:
                        if (!BlockStats[map.GetIDByCoord(this.bFrontX, this.bFrontY)].solid || WOptions.ignoreBorders) {
                            this.Trails();
                            this.position.x = this.bFrontX;
                            this.position.y = this.bFrontY;
                            moved = true;
                        }
                        else if (this.mode.autodigg) {
                            this.Digg();
                        }
                        break;
                }
                this.cachedBlockUnder = underBot;
            }

            if (moved) { this.cooldown = cd; this.HasMoved() }
            return this.cooldown;
        }
    }

    Trails() {
        let map = this.wrenderer.WorldMap;
        let underBot = map.GetIDByCoord(this.position.x, this.position.y);
        switch (underBot) {
            case Block.ground0:
                if (!RandInt(0, 9)) { map.SetTileWithCoords(this.position.x, this.position.y, Block.ground1) }
                break;
            case Block.ground1:
                if (!RandInt(0, 14)) { map.SetTileWithCoords(this.position.x, this.position.y, Block.ground2) }
                break;
        }
    }

    Rotate(side, fromUser = false, withCD = true) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            if (!this.inCooldown || !withCD) {
                switch (side) {
                    case "u":
                    case 0:
                        this.rotation = 0;
                        if (withCD) { this.cooldown = BotCDConstants.rotate };
                        break;
                    case "d":
                    case 2:
                        this.rotation = 180;
                        if (withCD) { this.cooldown = BotCDConstants.rotate };
                        break;
                    case "l":
                    case 3:
                        this.rotation = 270;
                        if (withCD) { this.cooldown = BotCDConstants.rotate };
                        break;
                    case "r":
                    case 1:
                        this.rotation = 90;
                        if (withCD) { this.cooldown = BotCDConstants.rotate };
                        break;
                }
            }
        }
    }

    randBukiva = ["v", "c", "w", "b", "g", "r"];

    Digg(fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            let timeNow = performance.now();
            if ((timeNow - (this.cdTime + this.cooldown)) > 0) {
                //console.log(timeNow-(this.cdTime + this.cooldown),this.rotation,CDConstants.digging);
                let [dx, dy] = [this.bFrontX, this.bFrontY];

                let map = this.wrenderer.WorldMap;
                let block = map.GetIDByCoord(dx, dy);

                if (BlockStats[block].hardness >= 0) {
                    map.DiggTile(dx, dy);
                }
                if (this.isVisible) {
                    this.wrenderer.finiteAnims.add(Anim.GetExemplar(AnimID.digg, new Vector2d(dx, dy), this.rotation));
                    this.wrenderer.dropAnims.add(DropAnim.GetExemplar(dx, dy, this.visibleposition, this.randBukiva[RandInt(0, 5)], RandInt(1, 400)));
                }

                this.cooldown = BotCDConstants.digging;
                return true;
            }
        }
    }

    SetBlock(fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            if (!this.inCooldown) {
                let [dx, dy] = [this.bFrontX, this.bFrontY];
                let map = this.wrenderer.WorldMap;
                let block = map.GetIDByCoord(dx, dy);
                switch (block) {
                    case 101:
                        map.SetTileWithCoords(dx, dy, 102); this.cooldown = BotCDConstants.setblock;
                        break
                    case 102:
                        map.SetTileWithCoords(dx, dy, 105); this.cooldown = BotCDConstants.setblock;
                        break
                    default:
                        if (BlockStats[block].replesable) {
                            map.SetTileWithCoords(dx, dy, 101);
                        }
                        this.cooldown = BotCDConstants.setblock;
                        break
                }
            }
        }
        //101,102,105
    }

    SetRoad(fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            if (!this.inCooldown) {
                let [dx, dy] = [this.bFrontX, this.bFrontY];
                let block = this.wrenderer.WorldMap.GetIDByCoord(dx, dy);

                if (block >= Block.ground0 && block <= Block.ground2) {
                    this.wrenderer.WorldMap.SetTileWithCoords(dx, dy, Block.road); this.cooldown = BotCDConstants.setblock;
                }
            }
        }
    }

    SetWB(fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            if (!this.inCooldown) {
                let [dx, dy] = [this.bFrontX, this.bFrontY];
                let block = this.wrenderer.WorldMap.GetIDByCoord(dx, dy);

                if (!BlockStats[block].solid) {
                    this.wrenderer.WorldMap.SetTileWithCoords(dx, dy, Block.block_war); this.cooldown = BotCDConstants.setblock;
                }
            }
        }
    }

    SetQuadro(fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            if (!this.inCooldown) {
                let [dx, dy] = [this.bFrontX, this.bFrontY];
                let block = this.wrenderer.WorldMap.GetIDByCoord(dx, dy);
                if (block == 49) {
                    this.wrenderer.WorldMap.SetTileWithCoords(dx, dy, 48); this.cooldown = BotCDConstants.setblock;
                }
                else if (BlockStats[block].replesable) {
                    this.wrenderer.WorldMap.SetTileWithCoords(dx, dy, 49); this.cooldown = BotCDConstants.setblock;
                }
            }
        }
        //49,48
    }

    UseGeo(fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            if (!this.inCooldown) {
                let [dx, dy] = [this.bFrontX, this.bFrontY];
                let block = this.wrenderer.WorldMap.GetIDByCoord(dx, dy);
                if (BlockStats[block].cantake) {
                    if (this.cargo.geoIsNoFull) {
                        this.cargo.PushGeo(block);
                        this.wrenderer.WorldMap.ClearTile(dx, dy);
                        this.cooldown = BotCDConstants.geo;
                        if (this.isVisible) this.wrenderer.finiteAnims.add(Anim.GetExemplar(AnimID.geo, new Vector2d(dx, dy), this.rotation));
                    }
                }
                else if (!BlockStats[block].solid) {
                    let gblock = this.cargo.PopGeo();
                    if (gblock) { this.wrenderer.WorldMap.SetTileWithCoords(dx, dy, gblock) }
                    this.cooldown = BotCDConstants.geo;
                }
            }
        }
    }

    Heal(fromUser = false) {
        if (!fromUser || this.programCondition.isManualControlAlloved) {
            if (!this.inCooldown) {
                this.cooldown = BotCDConstants.heal;
                if (this.isVisible) this.wrenderer.finiteAnims.add(Anim.GetExemplar(AnimID.heal, this.position, this.rotation));
            }
        }
    }
}

const TailStyles = Object.freeze([
    Object.freeze({ colors: ["#ff0d", "#f0fd", "#0ffd", "#fffd"], links: 5 }),
    Object.freeze({ colors: ["#f0fd", "#0ffd", "#fffd"], links: 5 }),
    Object.freeze({ colors: ["#f0f", "#ff0", "#f0f", "#ff0"], links: 5 }),
    Object.freeze({ colors: ["#fffd", "#fffd", "#fffd", "#fffd"], links: 5 }),
    Object.freeze({ colors: ["#000d", "#000d", "#000d", "#000d"], links: 5 }),
    Object.freeze({ colors: ["#fffd", "#000d", "#fffd", "#000d"], links: 5 }),
    Object.freeze({ colors: ["#7f0f", "#7f0f", "#7f0f", "#7f0f"], links: 5 }),
    Object.freeze({ colors: ["#d13f", "#d13f", "#d13f", "#d13f"], links: 5 }),
    Object.freeze({ colors: ["#fd0f", "#fd0f", "#fd0f", "#fd0f"], links: 5 }),
    Object.freeze({ colors: ["#0fff", "#0fff", "#0fff", "#0fff"], links: 5 }),
    Object.freeze({ colors: [RandColor16(), RandColor16(), RandColor16(), RandColor16(), RandColor16()], links: 5 }),
])

class Tails {
    /**
     * @param {Bot} targetbot 
     * @param {style} number
     */
    tail = [];
    constructor(targetbot, style = RandInt(0, TailStyles.length - 1)) {
        this.bot = targetbot;
        this.style = TailStyles[style % TailStyles.length];
        this.SetTail();
    }
    SetTail() {
        this.tail = new Array();

        for (let i = 0; i < this.style.colors.length; i++) {
            this.tail.push({ color: this.style.colors[i], pos: [] },);
            for (let j = 0; j < this.style.links; j++) {
                this.tail[i].pos.push(new Vector2d(this.bot.visibleposition.x, this.bot.visibleposition.y));
            }
        }
    }

    /**
     * @param {WRenderer} wrender 
     */
    Update(wrender) {
        for (let i = 0; i < this.tail.length; i++) {
            [this.tail[i].pos[0].x, this.tail[i].pos[0].y] = [this.bot.visibleposition.x, this.bot.visibleposition.y];
        }


        for (let j = 0; j < this.tail.length; j++) {
            if (this.bot.visibleposition.Distance(this.tail[j].pos[1]) > 5) {
                for (let i = 1; i < this.tail[j].pos.length; i++) {
                    this.tail[j].pos[i].CopyV2(this.tail[j].pos[i - 1]);
                }
            }
            for (let i = 1; i < this.tail[j].pos.length; i++) {
                this.tail[j].pos[i].x += RandInt(-3, 3) / 16;
                this.tail[j].pos[i].y += RandInt(-3, 3) / 16;
                this.tail[j].pos[i].Lerp2d(this.tail[j].pos[i - 1], 0.55, null, 0.0);
                //this.line(this.tail[j].pos[i],this.tail[j].pos[i-1],this.tail[j].color);
                wrender.Rey(this.tail[j].pos[i], this.tail[j].pos[i - 1], this.tail[j].color);
            }
        }
    }
}

/**
 * Представляет хранилище всех ботов
 */
class BotsContainer {
    length;
    chunks_count;
    /** 
     * @type {Map<Number,Bot>[]}
    */
    chunks;
    /**
     * @param {Number} width 
     * @param {Number} height 
     */
    constructor(width, height) {
        this.width = width;
        this.height = height;
        this.length = width * height;
        this.chunk_width = Math.ceil(width / 32);
        this.chunk_height = Math.ceil(height / 32);
        this.chunks_count = this.chunk_width * this.chunk_height;

        this.chunks = new Array(this.chunks_count);
        /** @type {Map<Number,{bot:Bot,lastCindex:Number}>} */
        this.botlist = new Map();
    }

    /**
     * @param {Vector2d} pos 
    */
    ChunkIndex(pos) {
        return (pos.x >> 5) + (pos.y >> 5) * this.chunk_width;
    }

    /**
     * @param {Number} x 
     * @param {Number} y 
     */
    ChunkIndex2(x, y) {
        return (x >> 5) + (y >> 5) * this.chunk_width;
    }

    /**
     * @param {Vector2d} position 
     * @param {Number} radius 
     */
    GetFromRadV2(position, radius = 0) {
        return this.GetFromRad(position.x, position.y, radius);
    }

    /**
     * @param {Number} x 
     * @param {Number} y
     * @param {Number} radius 
     * @returns {Set<Bot>}
     */
    GetFromRad(x, y, radius = 0) {
        let collection = new Set();
        if (radius) {
            let cx1 = (x - radius) >> 5;
            let cy1 = (y - radius) >> 5;
            let cx2 = (x + radius) >> 5;
            let cy2 = (y + radius) >> 5;
            for (let cy = cy1; cy <= cy2; cy++) {
                for (let cx = cx1; cx <= cx2; cx++) {
                    let chunk = this.GetChunk(cx, cy);
                    if (chunk?.size) {
                        chunk.forEach((val, key) => {
                            if (val.position.Distance2(x, y) <= radius) {
                                collection.add(val);
                            }
                        });
                    }
                }
            }
        }
        else {
            let chunk = this.GetChunk(x >> 5, y >> 5);
            if (chunk?.size) {
                chunk.forEach((val, key) => {
                    if (val.position.Distance2(x, y) <= radius) {
                        collection.add(val);
                    }
                });
            }
        }

        return collection;
    }

    GetFromBox(x, y) {
        let cx = x >> 5;
        let cy = y >> 5;

    }

    /**
     * @param {Bot|Bot[]} bot
     */
    Set(bot) {
        if (bot.length != undefined) {
            for (let i = 0; i < bot.length; i++) {
                this._Set(bot[i]);
            }
        }
        else {
            this._Set(bot);
        }
        return this;
    }

    /**@private */ _Set(/** @type {Bot} */ bot) {
        if (this.botlist.has(bot.id)) {
            console.warn("этот бот уже есть в списке", this.botlist, bot);
        }
        else {
            let ci = this.ChunkIndex(bot.position);
            this.botlist.set(bot.id, { bot: bot, lastCindex: ci });
            if (!this.chunks[ci]) { this.chunks[ci] = new Map() }

            this.chunks[ci].set(bot.id, bot);
            bot.onBotMoved = this._BotMoved.bind(this);
        }
    }

    /**
     * 
     * @param {Bot} bot 
     */
    /**@private */_BotMoved(bot) {
        let ci = this.ChunkIndex(bot.position);
        let bothere = this.chunks[ci]?.has(bot.id);
        if (!bothere) {
            let botdata = this.botlist.get(bot.id);
            this.chunks[botdata.lastCindex].delete(bot.id);

            botdata.lastCindex = ci;

            if (this.chunks[ci] == null) this.chunks[ci] = new Map();
            this.chunks[ci].set(bot.id, bot);
            //console.table(this.chunks);
        }
    }

    /**
     * @param {Bot} bot
     * @returns {Bot|null} возвращает удаленного со списка бота либо null если бот не найден
     */
    Delete(bot) {
        if (this.botlist.has(bot.id)) {
            let botdata = this.botlist.get(bot.id);
            this.chunks[botdata.lastCindex].delete(bot.id);
            this.botlist.delete(bot.id);
            botdata.bot.onBotMoved = null;
            return botdata.bot;
        }
        else {
            console.warn("этого бота уже нет в списке", this.botlist, bot);
            return null;
        }
    }

    /**
     * возвращает список всех ботов в чанке
     * @param {Number} cx 
     * @param {Number} cy
     */
    GetChunk(cx, cy) {
        if (cx >= 0 && cy >= 0 && cx < this.chunk_width && cy < this.chunk_height)
            return this.chunks[cx + cy * this.chunk_width];
        return undefined
    }
}