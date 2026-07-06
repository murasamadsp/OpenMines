class WorldMap {
    width = 0;
    height = 0;
    /** @type {Uint8Array} */ terrain;
    /** @type {Uint8Array} */ background;
    /** @type {Uint8Array} */ blocksMeta;
    /** @type {Set<Number>} */alivesList = new Set();

    /** @type {WRenderer} */ wRenderer;

    constructor(w = 32, h = 32) {
        this.Resyze(w, h);
    }

    /**
     * @param {number} w 
     * @param {number} h 
     */
    Resyze(w, h) {
        this.width = w;
        this.chunkWidth = Math.ceil(w / 32);
        this.height = h;
        this.chunkHeight = Math.ceil(h / 32);
        this.terrain = new Uint8Array(w * h);
        this.background = new Uint8Array(w * h);
        this.blocksMeta = new Uint8Array(w * h);
        this.packContainer = new PacksContainer(w, h);
        if (!this.geoPhisics) this.geoPhisics = new GeoPhisics(this);/////////////////////////////////////////////////////////////////////////////////////////обрабатывать пересоздание
        this.activeChunks = new Uint8Array(this.chunkWidth * this.chunkHeight);
        this.alivesList.clear();
        this.Fill(Block.ground0);
    }

    LoadWorld(wrld) {
        this.Resyze(wrld.width, wrld.height);
        let _data = wrld.data;
        let uintData = new Uint8Array(_data.length / 2);

        for (let i = 0; i < uintData.length; i++) {
            uintData[i] = parseInt(`${_data[i * 2]}${_data[i * 2 + 1]}`, 16);
        }

        let wi = 0;
        for (let i = 0; i < uintData.length; i++) {
            let cur = (uintData[i]);
            if (i < uintData.length - 2) {
                if (uintData[i + 1] == 0) {
                    //console.log(data[i+2]);

                    for (let j = 0; j < uintData[i + 2]; j++) {
                        this.SetTileWithIndex(wi, cur, RandInt(0, 10))
                        wi++;
                    }
                    i += 2;
                }
                else {
                    this.SetTileWithIndex(wi, cur, RandInt(0, 10))
                    wi++;
                }
            }
            else {
                this.SetTileWithIndex(wi, cur, RandInt(0, 10))
                wi++;
            }
        }
    }

    /**
     * @param {Number|Array<Number>} id 
     */
    Fill(id) {
        if (typeof (id) == "object") {
            for (let i = 0; i < this.terrain.length; i++) {
                let selectedID = id[Math.floor(Math.random() * id.length)];
                if (BlockStats[selectedID].solid) {
                    this.terrain[i] = selectedID;
                }
                else {
                    this.background[i] = selectedID;
                }
            }
        }
        else {
            for (let i = 0; i < this.terrain.length; i++) {
                if (BlockStats[id].solid) {
                    this.terrain[i] = id;
                }
                else {
                    this.background[i] = id;
                }
            }
        }
    }

    FillRect(id, xs, ys, w, h) {
        for (let y = ys; y < ys + h; y++) {
            for (let x = xs; x < xs + w; x++) {
                this.SetTileWithCoords(x, y, id);
            }
        }
    }

    ClearRect(xs, ys, w, h) {
        for (let y = ys; y < ys + h; y++) {
            for (let x = xs; x < xs + w; x++) {
                this.ClearTile(x, y, true);
            }
        }
    }

    /**
     * @param {Number} x 
     * @param {Number} y 
     */
    ValidCoords(x,y){
        return x >= 0 && x < this.width && y >= 0 && y < this.height;
    }

    SetTileWithCoords(x, y, id, density = 0) {
        if (this.ValidCoords(x,y))
            this._SetTile(x + y * this.width, id, density);
    }

    SetTileWithIndex(i, id, density = 0) {
        if (i < this.terrain.length)
            this._SetTile(i, id, density);
    }

    _SetTile(i, id, density) {
        if (BlockStats[id].solid) {
            if (BlockStats[id].is_alive) { this.alivesList.add(i); }
            this.terrain[i] = id;
            this.blocksMeta[i] = (BlockStats[id].hasdensity) ? density : 0;
        }
        else {
            this.background[i] = id;
        }
    }

    SetDensity(x, y, density) {
        if (this.ValidCoords(x,y))
            this.blocksMeta[x + y * this.width] = density;
    }

    GetDensity(x, y) {
        if (this.ValidCoords(x,y))
            return this.blocksMeta[x + y * this.width];
        return 0;
    }

    DiggTile(x, y) {
        let i = x + y * this.width;
        if (this.blocksMeta[i] > 0) {
            this.blocksMeta[i] -= 1;
        }
        else {
            this.terrain[i] = 0;
            this.blocksMeta[i] = 0;
        }
    }

    ClearTile(x, y, clearBackground = false) {
        let i = x + y * this.width;
        this.terrain[i] = 0;
        if (clearBackground) this.background[i] = Block.road;
        this.blocksMeta[i] = 0;
    }

    /**
     * Возвращает либо айди блока в слое твердых пород либо задник если в первом пусто
     * @param {number} x 
     * @param {number} y 
     * @returns {number} id
     */
    GetIDByCoord(x, y) {
        //console.log(x,y,this.terrain[x + y * this.width]);
        if (this.ValidCoords(x,y)) {
            let index = x + y * this.width;
            let tid = this.terrain[index];
            return (tid && BlockStats[tid].solid) ? tid : this.background[index];
        }
        return 104;
    }

    /**
     * 
     * @param {number} x 
     * @param {number} y 
     * @returns {number} id
     */
    GetIDByCoord_Terrain(x, y){
        if (this.ValidCoords(x,y)) {
            let index = x + y * this.width;
            return this.terrain[index];
        }
        return 104;
    }

    GetIDByCoord_Terrain_Unsafe(x, y){
        let index = x + y * this.width;
        return this.terrain[index];
    }

    /**
     * 
     * @param {number} x 
     * @param {number} y 
     * @returns {number} 
     */
    GetMetaByCoord(x, y) {
        //console.log(x,y,this.terrain[x + y * this.width]);
        if (this.ValidCoords(x,y)) {
            let index = x + y * this.width;
            return (this.blocksMeta[index]);
        }
        return 0;
    }
}