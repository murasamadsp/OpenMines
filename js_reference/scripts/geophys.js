class GeoPhisics {
    inProcess = false;
    /**
     * @param {WorldMap} wmap 
     */
    constructor(wmap) {
        this.wmap = wmap;
        this.Start();
    }

    Start() {
        let iter = this.wmap.alivesList[Symbol.iterator]();

        setInterval(() => {
            if (this.wmap.wRenderer) {
                this.SetActiveChunks();
            } else { console.warn("К родитерю WMap не привязан wRenderer(источник списка ботов) для выбора активных областей"); }

            this.FallingCycle();
            //this.AliveGrowth();
        }, 333);

        setInterval(() => {
            if (this.wmap.alivesList.size > 0 && RandInt(0, 1)) {
                for (let i = 0; i < 10; i++) {
                    let val = iter.next().value;
                    if (val != null) {
                        let result = this.AliveGrowth(val % this.wmap.width, ~~(val / this.wmap.width));
                        if (!result) { this.wmap.alivesList.delete(val) }
                    }
                    else {
                        iter = this.wmap.alivesList[Symbol.iterator]();
                    }
                }
            }
        }, 1000);
    }

    SetActiveChunks() {
        let bots = this.wmap.wRenderer.Bots;
        let curbot = this.wmap.wRenderer.CurrentBot;
        let activeChunks = wmap.activeChunks;

        //console.log(bots,activeChunks);
        let cwidth = wmap.chunkWidth;
        let cheight = wmap.chunkHeight;
        let cpos, bx, by;

        let ax1 = 1, ay1 = 1, ax2 = 1, ay2 = 1;
        for (let i = 0; i < bots.length; i++) {
            cpos = bots[i].position;

            if (bots[i] == curbot) {
                ax1 = 2; ay1 = 4; ax2 = 2; ay2 = 3;
            } else {
                if (!RandInt(0, 3)) {
                    ax1 = 1; ay1 = 1; ax2 = 1; ay2 = 0;
                } else {
                    ax1 = 0; ay1 = 1; ax2 = 0; ay2 = 0;
                }
            }

            bx = cpos.x >> 5;
            by = cpos.y >> 5;

            for (let y = by - ay1; y <= by + ay2; y++) {
                for (let x = bx - ax1; x <= bx + ax2; x++) {
                    if (x >= 0 && y >= 0 && x < cwidth && y < cheight) {
                        activeChunks[x + y * cwidth] = 1;
                    }
                }
            }
        }

    }

    DownFree(x, y) {
        let wmap = this.wmap;
        if (!BlockStats[wmap.GetIDByCoord(x, y + 1)].solid) {
            if (BlockStats[wmap.GetIDByCoord(x, y + 2)].falltype != null) {
                if (BlockStats[wmap.GetIDByCoord(x, y + 3)].solid) {
                    return 1;
                }
                return 2;
            }
            return 1;
        }
        if (BlockStats[wmap.GetIDByCoord(x, y + 1)].falltype != null) {
            return 0;
        }
        return 2
    }

    FallingCycle() {

        if (!this.inProcess) {
            this.inProcess = true;
            let activeCount = 0;
            let w = this.wmap.width;
            let h = this.wmap.height;
            let cwidth = wmap.chunkWidth;
            let cheight = wmap.chunkHeight;
            let activeChunks = wmap.activeChunks;
            for (let dy = cheight - 1; dy >= 0; dy--) {
                for (let dx = 0; dx < cwidth; dx++) {

                    if (activeChunks[dx + dy * cwidth]) {
                        activeCount++;
                        for (let y = (dy << 5) + 31; y >= (dy << 5); y--) {
                            for (let x = dx << 5; x < (dx << 5) + 32 && x < w; x++) {

                                let cblock = this.wmap.GetIDByCoord_Terrain(x, y);
                                let btype = BlockStats[cblock].falltype;
                                if (btype != null) {
                                    let wmap = this.wmap;
                                    if (btype == FallType.bolder) {
                                        let df = this.DownFree(x, y);
                                        if (df) {
                                            if (df == 1) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x, y + 1, cblock);
                                            }
                                        }
                                        else if (RandInt(0, 1) == 0) {
                                            if (!BlockStats[wmap.GetIDByCoord_Terrain(x + 1, y + 1)].solid && !BlockStats[wmap.GetIDByCoord_Terrain(x + 1, y)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x + 1, y + 1, cblock);
                                            }
                                            else if (!BlockStats[wmap.GetIDByCoord_Terrain(x - 1, y + 1)].solid && !BlockStats[wmap.GetIDByCoord_Terrain(x - 1, y)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x - 1, y + 1, cblock);
                                            }
                                        }
                                        else {
                                            if (!BlockStats[wmap.GetIDByCoord_Terrain(x - 1, y + 1)].solid && !BlockStats[wmap.GetIDByCoord_Terrain(x - 1, y)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x - 1, y + 1, cblock);
                                            }
                                            else if (!BlockStats[wmap.GetIDByCoord_Terrain(x + 1, y + 1)].solid && !BlockStats[wmap.GetIDByCoord_Terrain(x + 1, y)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x + 1, y + 1, cblock);
                                            }
                                        }
                                    } else {
                                        let df = this.DownFree(x, y);
                                        if (df) {
                                            if (df == 1) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x, y + 1, cblock);
                                            }
                                        }
                                        else if (RandInt(0, 1) == 0) {
                                            if (!BlockStats[wmap.GetIDByCoord_Terrain(x + 1, y + 1)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x + 1, y + 1, cblock);
                                            }
                                            else if (!BlockStats[wmap.GetIDByCoord_Terrain(x - 1, y + 1)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x - 1, y + 1, cblock);
                                            }
                                        }
                                        else {
                                            if (!BlockStats[wmap.GetIDByCoord_Terrain(x - 1, y + 1)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x - 1, y + 1, cblock);
                                            }
                                            else if (!BlockStats[wmap.GetIDByCoord_Terrain(x + 1, y + 1)].solid) {
                                                wmap.ClearTile(x, y);
                                                wmap.SetTileWithCoords(x + 1, y + 1, cblock);
                                            }
                                        }
                                    }
                                }
                            }
                        }


                        activeChunks[dx + dy * cwidth] = 0;
                    }
                }
            }
            //console.log(activeCount,activeCount*1024);
        }
        this.inProcess = false;
    }

    CalculateAliveProductHardness(x, y, block) {
        let hardnessMul = 1;
        if (this.wmap.GetIDByCoord_Terrain(x - 1, y) == Block.rock_hypno) hardnessMul++;
        if (this.wmap.GetIDByCoord_Terrain(x + 1, y) == Block.rock_hypno) hardnessMul++;
        if (this.wmap.GetIDByCoord_Terrain(x, y + 1) == Block.rock_hypno) hardnessMul++;
        if (this.wmap.GetIDByCoord_Terrain(x, y - 1) == Block.rock_hypno) hardnessMul++;
        return AliveProductHardness[block] * hardnessMul;
    }

    IsBlackRockNear(x, y) {
        if (this.wmap.GetIDByCoord_Terrain(x - 1, y - 1) == Block.rock_black) return true;
        if (this.wmap.GetIDByCoord_Terrain(x, y - 1) == Block.rock_black) return true;
        if (this.wmap.GetIDByCoord_Terrain(x + 1, y - 1) == Block.rock_black) return true;
        if (this.wmap.GetIDByCoord_Terrain(x - 1, y) == Block.rock_black) return true;
        if (this.wmap.GetIDByCoord_Terrain(x + 1, y) == Block.rock_black) return true;
        if (this.wmap.GetIDByCoord_Terrain(x - 1, y + 1) == Block.rock_black) return true;
        if (this.wmap.GetIDByCoord_Terrain(x, y + 1) == Block.rock_black) return true;
        if (this.wmap.GetIDByCoord_Terrain(x + 1, y + 1) == Block.rock_black) return true;
        return false
    }

    SetIfFree(x, y, block, h) {
        if (!BlockStats[this.wmap.GetIDByCoord_Terrain(x, y)].solid) { this.wmap.SetTileWithCoords(x, y, block, h) }
    }

    AliveGrowth(x, y) {
        let block = this.wmap.GetIDByCoord_Terrain(x, y);
        if (BlockStats[block].is_alive) {
            if (AliveProductHardness[block]) {
                switch (block) {
                    case Block.alive_cyan: {
                        let h = this.CalculateAliveProductHardness(x, y, block);
                        this.SetIfFree(x - 1, y, Block.cry_cyan, h);
                        this.SetIfFree(x + 1, y, Block.cry_cyan, h);
                        this.SetIfFree(x, y - 1, Block.cry_cyan, h);
                        this.SetIfFree(x, y + 1, Block.cry_cyan, h);
                        //console.log(x, y, block, h);
                    }
                        break;
                    case Block.alive_red:
                        if (this.IsBlackRockNear(x, y)) {
                            let h = this.CalculateAliveProductHardness(x, y, block);
                            this.SetIfFree(x - 1, y, Block.cry_red, h);
                            this.SetIfFree(x + 1, y, Block.cry_red, h);
                            this.SetIfFree(x, y - 1, Block.cry_red, h);
                            this.SetIfFree(x, y + 1, Block.cry_red, h);
                        }
                        break;
                    case Block.alive_vio:
                        if (this.IsBlackRockNear(x, y)) {
                            let h = this.CalculateAliveProductHardness(x, y, block);
                            this.SetIfFree(x - 1, y, Block.cry_vio, h);
                            this.SetIfFree(x + 1, y, Block.cry_vio, h);
                            this.SetIfFree(x, y - 1, Block.cry_vio, h);
                            this.SetIfFree(x, y + 1, Block.cry_vio, h);
                        }
                        break;
                    case Block.alive_black:

                        break;
                    case Block.alive_white:
                        if (this.wmap.GetIDByCoord(x, y - 1) == Block.magma) {
                            let h = this.CalculateAliveProductHardness(x, y, block);
                            this.SetIfFree(x - 1, y, Block.cry_white, h);
                            this.SetIfFree(x + 1, y, Block.cry_white, h);
                            this.SetIfFree(x, y + 1, Block.cry_white, h);
                            this.SetIfFree(x + 1, y + 1, Block.cry_white, h);
                            this.SetIfFree(x + 1, y - 1, Block.cry_white, h);
                            this.SetIfFree(x + 1, y - 1, Block.cry_white, h);
                            this.SetIfFree(x - 1, y + 1, Block.cry_white, h);

                            this.wmap.ClearTile(x, y - 1, false);
                        }
                        break;
                    case Block.alive_reinbow:

                        break;
                    case Block.alive_blue:
                        if (this.wmap.GetDensity(x, y) > 0) {
                            this.wmap.SetDensity(x, y, 0);
                        } else {
                            let moveDir = RandInt(0, 3);
                            let mx = 0, my = 0;
                            switch (moveDir) {
                                case 0: [mx, my] = [x, y + 1]; break;
                                case 1: [mx, my] = [x, y - 1]; break;
                                case 2: [mx, my] = [x + 1, y]; break;
                                case 3: [mx, my] = [x - 1, y]; break;
                            }
                            let place = this.wmap.GetIDByCoord_Terrain(mx, my);

                            if (!BlockStats[place].solid) {
                                let h = this.CalculateAliveProductHardness(x, y, block);
                                this.wmap.SetTileWithCoords(mx, my, Block.alive_blue);
                                this.wmap.SetDensity(mx, my, 1);
                                this.wmap.SetTileWithCoords(x, y, Block.cry_blue, h);
                            }
                        }
                        break;
                    default:
                        console.warn("Проебочка");
                        break;
                }
            }
        }
        else {
            return false
        }
        return true;
    }
}


const AliveProductHardness = Object.freeze({
    50: 1,//"голубая жива";
    51: 2,//"красная жива";
    52: 1,//"фиол жива";
    53: 1,//"черная жива";
    54: 8,//"белая жива";
    55: 1,//"радужная жива";
    116: 20,//синяя жива
})