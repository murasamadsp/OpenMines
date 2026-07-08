class Resp extends Pack {
    static __consumeCry = "b";
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-----",
            "-fff-",
            "-f1e-",
            "-fff-",
            "-f2f-",
            "-----",
        ]
    )

    selfCargo = new PackCargo();

    /** @param {Bot} bot @param {Vector2d} position */
    constructor(bot, position) {
        super(bot, position);

    }
}

class Stock extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-----",
            "-fff-",
            "-f1f-",
            "-----",
        ]
    )

    /** @param {Bot} bot @param {Vector2d} position */
    constructor(bot, position) {
        super(bot, position);
        this.cargo = new CryCargo();
    }
}

class Craft extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-----",
            "-cfc-",
            "-f1f-",
            "-fef-",
            "-----",
        ]
    )
    /** @param {Bot} bot @param {Vector2d} position */
    constructor(bot, position) {
        super(bot, position);
    }
}

class Gun extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-----",
            "-f-f-",
            "--1--",
            "-f-f-",
            "-----",
        ],
        Block.empty
    )

    static __consumeCry = "c";
    static __onlyClanpack = true;

    /** @param {Bot} bot @param {Vector2d} position */
    constructor(bot, position) {
        super(bot, position);
    }
}

class Gate extends Pack {
    static __interactive = false;

    /** @param {Bot} bot @param {Vector2d} position */
    constructor(bot, position) {
        super(bot, position);
    }

    /**свой епты уникальный чек @param {Number} cx  @param {Number} cy  @param {WorldMap} wmap */
    BeforeInstalationChack(cx, cy, wmap) {

    }
}

class Market extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-------",
            "-cfefc-",
            "-ffeff-",
            "-ee1ee-",
            "-ffeff-",
            "-cfefc-",
            "-------",
        ]
    )
    static __onlyNoClanPack = true;
}

class UP extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-----",
            "-cfc-",
            "-fff-",
            "-f1f-",
            "-fef-",
            "-----",
        ]
    )
    static __onlyNoClanPack = true;
}

class TP extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-----",
            "-fff-",
            "-f1f-",
            "-fef-",
            "-----",
        ]
    )
}

class Clans extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "-------",
            "-cfffc-",
            "-ff1ff-",
            "-ffeff-",
            "-cfefc-",
            "-------",
        ]
    )
}

class Science extends Pack {
    static __construction = PackConstruction.GetBlockGrid(
        [
            "---fffff---",
            "--fff1fff--",
            "-cfffefffc-",
            "-fff---fff-",
            "-cfc---cfc-",
            "-----------",
        ]
    )
}


/**
 * Представляет хранилище всех зданий в мире, доступ по мировым координатам любого из входов в здание
 */
class PacksContainer {
    width;
    height;
    chunk_width;
    chunk_height;
    length;
    chunks_count;
    /** @type {Map<Number,Pack>[]} */
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
    }

    Get(x, y) {
        if (x >= 0 && x < this.width && y >= 0 && y < this.height) {
            let [cx, cy] = [x >> 5, y >> 5];
            let chunk = this.chunks[cx + cy * this.chunk_width];
            if (chunk) {
                let pack = chunk.get(x + y * this.width);
                if (!pack) { console.warn(x, y, chunk, "попытка найти пак там где его нет") }
                return pack;
            }
            else {
                console.error(x, y, chunk, "попытка найти пак в пустом чанке");
            }
        }
        return undefined;
    }

    /**
     * @param {Pack} pack 
     * @param {Number} x 
     * @param {Number} y 
     */
    Set(pack, x, y) {
        if (x >= 0 && x < this.width && y >= 0 && y < this.height) {
            let [cx, cy] = [x >> 5, y >> 5];
            let index = x + y * this.width;
            let cindex = cx + cy * this.chunk_width;
            let chunk = this.chunks[cindex];
            if (chunk) {
                if (chunk.get(index)) {
                    console.error("Попытка записать пак поверх другого", pack, chunk.get(index));
                }
                else {
                    chunk.set(index, pack);
                }
            }
            else {
                chunk = new Map();
                chunk.set(index, pack);
                this.chunks[cindex] = chunk;
            }
        }
    }

    /**
     * @param {Number} cx 
     * @param {Number} cy
     */
    GetChunk(cx, cy) {
        if (cx >= 0 && cx < this.chunk_width && cy >= 0 && cy < this.chunk_height) {
            return this.chunks[cx + cy * this.chunk_width];
        }
        console.warn("попытка считать чанк вне зоны доступа", x, y);
        return undefined;
    }
}



let r = new Resp(null, new Vector2d(100, 100));
let p = new Pack(null, new Vector2d(100, 100));
let g = new Gate(null, new Vector2d(100, 100));
let s = new Stock(null, new Vector2d(100, 100));
let tp = new TP(null, new Vector2d(100, 100));
let up = new UP(null, new Vector2d(100, 100));
let market = new Market(null, new Vector2d(100, 100));
let craft = new Craft(null, new Vector2d(100, 100))
let gun = new Gun(null, new Vector2d(100, 100));
let science = new Science(null, new Vector2d(100, 100));