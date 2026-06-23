const EntityType = Object.freeze({
    pack: 0,
    temporal: 1
})

class Entity {

}

class PackConstruction {
    static dictionary = Object.freeze({
        "-": Block.empty,
        "f": Block.build_frame,
        "c": Block.build_corner,
        "e": Block.build_entry,
        "r": Block.road,
        "g": Block.g_road,
    })

    /**
     * 
     * @param {String[][]} plan 
     */
    static GetBlockGrid(plan,replasenum = Block.build_entry) {
        if (plan) {
            /** @type {Array<Uint8Array>} */                let grid = new Array();
            /** @type {{x:Number,y:Number,id:Number}[]} */  let interactives = new Array();
            for (let y = 0; y < plan[0].length; y++) {
                let line = new Uint8Array(plan.length);
                for (let x = 0; x < plan.length; x++) {
                    let p = plan[x][y];
                    let blockid = PackConstruction.dictionary[p];

                    if (blockid == undefined) {
                        let num = parseInt(plan[x][y]);
                        if (num != NaN) {
                            interactives.push({ x: x, y: y, id: num });
                            blockid = replasenum;
                        }
                        else {
                            blockid = 0;
                        }
                    }

                    line[x] = blockid;
                }
                grid.push(line);
            }

            return {
                grid: grid,
                width: plan[0].length,
                height: plan.length,
                interactivePoints: interactives,
            };
        }
        else console.error("указан неправильный шаблон конструкции", plan);
    }


}

class Pack extends Entity {
    /** @type {Number} */
    owner;
    /** @type {Number} */
    clan;
    /**@type {Number} время в миллисекундах*/
    createTime;

    /**@type {PackCargo} время в миллисекундах*/
    selfCargo;

    /**@type {Number} */
    hp = 1000;
    /**@type {Number} */
    hpmax = 1000;

    /** @type {Vector2d} */
    position;

    static __modulesAllowed = [];
    get modulesAllowed() { return Object.getPrototypeOf(this).constructor.__modulesAllowed }
    static __onlyClanpack = false;
    get onlyClanpack() { return Object.getPrototypeOf(this).constructor.__onlyClanpack }
    static __onlyNoClanPack = false;
    get onlyNoClanPack() { return Object.getPrototypeOf(this).constructor.__onlyNoClanPack }
    /**@type {String} */
    static __consumeCry = null;
    get consumeCry() { return Object.getPrototypeOf(this).constructor.__consumeCry }
    static __interactive = true;
    /** @type {Boolean} */
    get interactive() { return Object.getPrototypeOf(this).constructor.__interactive }

    /** @type {{grid:Uint8Array[],width:Number,height:Number}} */
    static __construction;
    /** @type {{grid:Uint8Array[],width:Number,height:Number}} */
    get construction() { return Object.getPrototypeOf(this).constructor.__construction }

    /** @param {Bot} bot @param {Vector2d} position */
    constructor(bot, position) {
        super();
        this.clan = bot?.clanID != null ? bot.id : -1;
        this.owner = bot?.id != null ? bot.id : -1;
        this.position = position ? position : console.error("не указаны координаты для пака");
        this.createTime = Date.now();
    }

    /**
     * 
     * @param {Bot} bot 
     */
    Interact(bot) {
        if (this.interactive) {

        }
    }

    /**
     * 
     * @param {Number} cx 
     * @param {Number} cy 
     * @param {WorldMap} wmap 
     */
    Construct(cx, cy, wmap) {
        let constr = this.construction;
        let [dx, dy] = [~~(constr.width / 2), ~~(constr.height / 2)]
        for (let y = cy - dy, by = 0; y <= cy + dy; y++, by++) {
            for (let x = cx - dx, bx = 0; x <= cx + dx; x++, bx++) {
                if (constr.grid[bx][by]) {
                    wmap.SetTileWithCoords(x, y, constr.grid[bx][by]);
                }
            }
        }
    }

    /**
     * Проверка на валидность места для установки пака
     * @param {Number} cx 
     * @param {Number} cy 
     * @param {WorldMap} wmap 
     * @returns {Array<{x:Number,y:Number}>} список координат блоков где блокируется стройка
     */
    BeforeInstalationChack(cx, cy, wmap) {
        let constr = this.construction;
        let [dx, dy] = [~~(constr.width / 2), ~~(constr.height / 2)]
        let blockList = new Array;
        for (let y = cy - dy; y < cy + dy; y++) {
            for (let x = cx - dx; x < cx + dx; x++) {
                let block = wmap.GetIDByCoord(x, y);
                if (BlockStats[block].solid) {
                    blockList.push({ x: x, y: y, block: block });
                }
            }
        }
        return blockList;
    }
}

class Temporal extends Entity {

}