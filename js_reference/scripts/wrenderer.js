class WRenderer {
    scale = 1;
    blockSize = 16 * this.scale;

    Camera = Object.freeze({
        position: new Vector2d(50, 50),
    });
    GetCameraCenter() {
        return { x: ~~(WorldCanv.width >> 1 / this.blockSize), y: ~~(WorldCanv.height >> 1 / this.blockSize) };
    }
    GetCameraCenterBlockPos() {
        return { x: ~~((WorldCanv.width >> 1) / this.blockSize + this.Camera.position.x), y: ~~((WorldCanv.height >> 1) / this.blockSize + this.Camera.position.y) };
    }

    renderTime = 0;
    elapsed = 0;
    elapsedPrev = 0;
    deltatime = 0;

    frameCount = 0;

    /** @type {Set<Anim>} */
    finiteAnims = new Set();
    /** @type {Set<DropAnim>} */
    dropAnims = new Set();
    /** @type {Set<LightningAnim>} */
    lightningAnims = new Set();


    /**@type {OffscreenRenderingContext} */ TerrainCtx;

    WorldMap;
    /**
     * @param {OffscreenRenderingContext} Tctx
     * @param {WorldMap} world
     * @param {Array<Bot>} bots
     */
    constructor(Tctx, world, bots = []) {
        world.wRenderer = this;
        this.WorldMap = world;
        this.TerrainCtx = Tctx;
        this.Bots = bots;
        if (bots.length) this.CurrentBot = bots[0];
        this.StartRenderCycle();
    }

    MouseSetBlock(_x, _y, id, radius = 0) {
        let dx1 = Math.floor(_x / this.blockSize + this.Camera.position.x);
        let dy1 = Math.floor(_y / this.blockSize + this.Camera.position.y);
        for (let y = dy1 - radius; y <= dy1 + radius; y++) {
            for (let x = dx1 - radius; x <= dx1 + radius; x++) {
                if (id) { this.WorldMap.SetTileWithCoords(x, y, id) } else { this.WorldMap.ClearTile(x, y) }
            }
        }
    }

    SpawnNewRandomBot(count = 1) {
        for (let i = 0; i < count; i++) {
            this.Bots.push(new Bot(new Vector2d().CopyV2(this.CurrentBot?.position ?? new Vector2d(50, 50)), 2, null, this));
        }
    }


    StartRenderCycle() {
        let renderDelta = this.Update();
        this.frameCount++;
        //window.requestAnimationFrame(this.StartRenderCycle.bind(this));

        setTimeout(() => {
            this.StartRenderCycle();
        }, (1000 / WOptions.fps) - renderDelta);
    }

    Update() {
        this.elapsedPrev = this.elapsed;
        this.elapsed = performance.now();
        this.deltatime = this.elapsed - this.elapsedPrev;

        for (let i = 0; i < 2; i++) {
            this.WorldMap.SetTileWithCoords(RandInt(35, 75), RandInt(70, 81), RandInt(0, 1) ? RandInt(60, 68) : RandInt(40, 45));////////////////////////////////////////////////////////////////////////////////////////////////
        }

        if (!WOptions.ignoreBorders) {
            for (let i = 0; i < this.Bots.length; i++) {
                if (!RandInt(0, 10))
                    this.Bots[i].CheckBlockUnder();
            }
        }

        if (this.Bots.length) this.GotoCurrentBot();
        this.DrawTerrain();
        if (this.Bots.length) {
            this.DrawBots();
            this.DrawFiniteAnims();
            this.DrawDropsAnim();
            this.BotInfo();
        }

        let stop = performance.now();
        return this.renderTime = stop - this.elapsed;
    }

    DrawFiniteAnims() {
        this.finiteAnims.forEach((curAnim, val2) => {
            let curframe = curAnim.GetCurrentFrame();
            if (!curAnim.IsEnded && curframe) {
                let dx = curAnim.position.x - this.Camera.position.x;
                let dy = curAnim.position.y - this.Camera.position.y;

                this.TerrainCtx.save();
                this.TerrainCtx.translate(Math.floor(dx * 16 + 8), Math.floor(dy * 16 + 8));
                this.TerrainCtx.rotate(curAnim.rotation * Math.PI / 180);
                this.TerrainCtx.drawImage(curframe.src, curframe.frame.x, curframe.frame.y, curframe.frame.w, curframe.frame.h, -8, -8, 16 * Animations[curAnim.id].syzeInBlocks.w, 16 * Animations[curAnim.id].syzeInBlocks.h);

                this.TerrainCtx.restore();
            }
            else {
                this.finiteAnims.delete(curAnim);
            }
        })
    }

    DrawDropsAnim() {
        this.dropAnims.forEach((curAnim, val2) => {
            this.TerrainCtx.font = "16px Serif";
            if (!curAnim.isFinished) {
                let dx = curAnim.position.x - this.Camera.position.x;
                let dy = curAnim.position.y - this.Camera.position.y;
                curAnim.Update();
                this.TerrainCtx.fillStyle = curAnim.color;
                this.TerrainCtx.fillText(`${curAnim.text}`, Math.floor(dx * 16 + 8.5), Math.floor(dy * 16 + 8.5));
            }
            else {
                this.dropAnims.delete(curAnim);
            }
        })
    }

    DrawLightings(){
        this.lightningAnims.forEach((curAnim, val2) => {
            if (!curAnim.isFinished) {
                curAnim.Update();
            }
            else{
                this.lightningAnims.delete(curAnim)
            }
        })
    }

    /**
     * Рисует линию без привязки к мировым координатам (просто пиксельные координаты)
     * @param {Vector2d} v1 
     * @param {Vector2d} v2 
     * @param {String} color 
     */
    line(v1, v2, color = "#ff0") {
        this.TerrainCtx.strokeStyle = color;
        this.TerrainCtx.beginPath();
        this.TerrainCtx.moveTo(v1.x, v1.y);
        this.TerrainCtx.lineTo(v2.x, v2.y);
        this.TerrainCtx.stroke()
    }

    DrawTerrain() {
        let xm = this.Camera.position.x_mantice * 16;
        let ym = this.Camera.position.y_mantice * 16;
        let cx = this.Camera.position.x_int;
        let cy = this.Camera.position.y_int;
        let mapx, mapy;
        for (let y = 0; y < Math.ceil(WorldCanv.height / 16) + 1; y++) {
            for (let x = 0; x < Math.ceil(WorldCanv.width / 16) + 1; x++) {
                mapx = cx + x;
                mapy = cy + y;
                this.SetTile(Math.floor(x * 16 - xm), Math.floor(y * 16 - ym), this.WorldMap.GetIDByCoord(mapx, mapy), { x: mapx, y: mapy })
            }
        }

    }

    SetTile(tx, ty, tid, worldPos) {
        let data = WorldTileset.TilesData[tid];
        let tyleSyzing;
        let syze = this.blockSize;
        if (data == null) {
            tyleSyzing = WorldTileset.TilesData[0].tyles;
        }
        else {
            tyleSyzing = data.tyles;
        }

        let dx, dy;

        dx = ((worldPos.x * syze) % tyleSyzing.w);
        dy = ((worldPos.y * syze) % tyleSyzing.h);

        this.TerrainCtx.drawImage(WorldTileset.sourceImage, tyleSyzing.x + dx, tyleSyzing.y + dy, syze, syze, tx, ty, syze, syze);
        if (data && data.filter) {
            let pn = performance.now() / 1000;
            pn = pn - Math.floor(pn);
            if (pn > 0.5) { this.TerrainCtx.drawImage(WorldTileset.sourceImage, data.filter.x + dx, data.filter.y + dy, syze, syze, tx, ty, syze, syze); }
        }
    }

    DrawBots() {
        for (let i = 0; i < this.Bots.length; i++) {
            let bot = this.Bots[i];
            let sid = bot.skin;
            bot.isVisible = false;

            bot.visibleposition.Lerp2d(bot.position, 0.18);
            bot.visiblerotation = Lerp(bot.visiblerotation, bot.rotation, 0.1);

            let dx = bot.visibleposition.x - this.Camera.position.x;
            let dy = bot.visibleposition.y - this.Camera.position.y;

            if (dx > -4 && dy > -4 && dx <= WorldCanv.width / 16 + 4 && dy <= WorldCanv.height / 16 + 4) {
                bot.isVisible = true;

                bot.tail.Update(this);

                if (WOptions.vectors) {
                    this.Rey(bot.position, bot.visibleposition);
                    this.Rey2(bot.position, bot.rotation == 90 ? 0.3 : bot.rotation == 270 ? -0.3 : 0, bot.rotation == 0 ? -0.3 : bot.rotation == 180 ? 0.3 : 0, "#0f36");
                }

                this.TerrainCtx.save();
                this.TerrainCtx.translate(Math.floor(dx * 16 + 8), Math.floor(dy * 16 + 8));
                this.TerrainCtx.rotate(bot.visiblerotation * Math.PI / 180);

                this.TerrainCtx.drawImage(SkinsSet.SkinsIMG, SkinsSet.skins[sid].x, SkinsSet.skins[sid].y, 32, 32, -8, -8, 16, 16);

                this.TerrainCtx.restore();

                //this.TerrainCtx.fillStyle = "#288e";
                //this.TerrainCtx.font = "10px  sans-serif";
                //this.TerrainCtx.fillText(`${bot.id} ${bot.position.x.toFixed(1)}:${bot.position.y.toFixed(1)} r:${bot.visiblerotation.toFixed(1)}  ${(dx*16).toFixed(1)}  ${(dy*16).toFixed(1)}`, dx*16+16, dy*16+25);

                if (WOptions.nicknames) {
                    this.TerrainCtx.fillStyle = bot.programCondition.isExecute ? bot.programCondition.isHandModeActive ? "#0d2" : "#dd4" : "#fff";
                    this.TerrainCtx.font = "10px  sans-serif";

                    let nickid = `[${bot.id}]${bot.nickName}`;
                    let nlength = this.TerrainCtx.measureText(nickid).width;
                    this.TerrainCtx.fillText(nickid, Math.floor(dx * 16 - nlength / 2 + 8 + 0.5), Math.floor(dy * 16 - 8 + 0.5));
                }

                if (WOptions.viewBox) {
                    let voffset = bot.programCondition.stack[bot.programCondition.stackDepth].viewOffset;
                    this.Box(new Vector2d(bot.position.x + voffset.x, bot.position.y + voffset.y));
                }
            }
        }
    }

    GotoCurrentBot() {
        if (!this.CurrentBot) { this.CurrentBot = this.Bots[0] }
        let [cdx, cdy] = [WorldCanv.width / 2 / 16, WorldCanv.height / 2 / 16];

        this.Camera.position.Lerp2d(this.CurrentBot.position, (0.9 / this.deltatime), new Vector2d(cdx, cdy), 1 / 16);
    }

    /**
     * рисует линию между точками мировых координат с учетом смещения и масштаба камеры
     * @param {Vector2d} v1 
     * @param {Vector2d} v2
     * @param {string} [color="#0f04"] 
     */
    Rey(v1, v2, color = "#0f04") {
        if (this.Rey.prevcolor === undefined) { this.Rey.prevcolor = color }
        else {
            if (this.Rey.prevcolor != color) {
                this.Rey.prevcolor = color;
                this.TerrainCtx.strokeStyle = color;
            }
        }
        let halfBS = this.blockSize / 2 + 0.5;
        let dx1 = (v1.x - this.Camera.position.x) * this.blockSize + halfBS;
        let dy1 = (v1.y - this.Camera.position.y) * this.blockSize + halfBS;
        let dx2 = (v2.x - this.Camera.position.x) * this.blockSize + halfBS;
        let dy2 = (v2.y - this.Camera.position.y) * this.blockSize + halfBS;

        this.TerrainCtx.strokeStyle = color;
        this.TerrainCtx.beginPath();
        this.TerrainCtx.moveTo(dx1, dy1);
        this.TerrainCtx.lineTo(dx2, dy2);
        this.TerrainCtx.stroke()
    }

    Box(v, color = "#1ffa") {
        this.TerrainCtx.strokeStyle = color;

        let dx1 = (v.x - this.Camera.position.x) * this.blockSize + 0.5;
        let dy1 = (v.y - this.Camera.position.y) * this.blockSize + 0.5;
        let dx2 = dx1 + this.blockSize;
        let dy2 = dy1 + this.blockSize;

        this.TerrainCtx.strokeStyle = color;
        this.TerrainCtx.beginPath();
        this.TerrainCtx.moveTo(dx1, dy1);

        this.TerrainCtx.lineTo(dx1, dy2);
        this.TerrainCtx.lineTo(dx2, dy2);
        this.TerrainCtx.lineTo(dx2, dy1);
        this.TerrainCtx.lineTo(dx1, dy1);

        this.TerrainCtx.stroke()
    }

    Rey2(Vstart, dx, dy, color = "#0f04") {
        this.Rey(Vstart, new Vector2d(Vstart.x + dx, Vstart.y + dy), color);
    }

    BotInfo() {
        let bot = this.CurrentBot;
        //this.TerrainCtx.font = "15px Serif";
        this.TerrainCtx.fillStyle = "#fff";
        // this.TerrainCtx.fillText(`${bot.nickName}     HP:${bot.stats.hpNow}/${bot.stats.hpMax}`, 20, 20);

        // this.TerrainCtx.fillText(`Груз:[${bot.cargo.GetVolume()}]`, 20, 40);
        // this.TerrainCtx.fillText(`g:${bot.cargo.g}  [${bot.cargo.GetVolume("g")}]`, 20, 60);
        // this.TerrainCtx.fillText(`b:${bot.cargo.b}  [${bot.cargo.GetVolume("b")}]`, 20, 80);
        // this.TerrainCtx.fillText(`r:${bot.cargo.r}  [${bot.cargo.GetVolume("r")}]`, 20, 100);
        // this.TerrainCtx.fillText(`w:${bot.cargo.w}  [${bot.cargo.GetVolume("w")}]`, 20, 120);
        // this.TerrainCtx.fillText(`v:${bot.cargo.v}  [${bot.cargo.GetVolume("v")}]`, 20, 140);
        // this.TerrainCtx.fillText(`c:${bot.cargo.c}  [${bot.cargo.GetVolume("c")}]`, 20, 160);

        //this.TerrainCtx.fillText(`X:${bot.position.x} Y:${bot.position.y}`, 20, 200);

        this.TerrainCtx.font = "14px Serif";
        //let nlength = this.TerrainCtx.measureText(nickid).width;
        if (bot.cargo.geo) {
            let geotext = "-";
            for (let i = bot.cargo.geoFilledValue - 1, i2 = 0; i >= 0 && i2 < 5; i--, i2++) {
                geotext += bot.cargo.geo[i] + "-";
            }
            this.TerrainCtx.fillText(`Geo:[${geotext}]${bot.cargo.geoFilledValue}/${bot.cargo.geo.length}`, 25, 550);
        }

        this.TerrainCtx.font = "10px Serif";
        let [x, y] = [bot.position.x, bot.position.y];
        this.TerrainCtx.fillStyle = "#0007";
        this.TerrainCtx.fillRect(25, 290, 60, 55);


        for (let dy = -1; dy <= 1; dy++) {
            for (let dx = -1; dx <= 1; dx++) {
                this.TerrainCtx.fillStyle = "#fff";
                this.TerrainCtx.fillText(`${this.WorldMap.GetIDByCoord(x + dx, y + dy)}`, 50 + 20 * dx, 320 + 20 * dy);
                this.TerrainCtx.fillStyle = "#0f0a";
                this.TerrainCtx.fillText(`${this.WorldMap.GetMetaByCoord(x + dx, y + dy)}`, 50 + 20 * dx, 320 + 10 + 20 * dy);
            }
        }
        this.TerrainCtx.fillStyle = "#fff";
        this.TerrainCtx.font = "16px Serif";
        this.TerrainCtx.fillText(`delta${this.deltatime.toFixed(0)} render:${this.renderTime.toFixed(1)}`, 10, WorldCanv.height - 10);

        if (this.frameCount % 15 == 0) { guiInventory.Update(this.CurrentBot) }
        if (this.frameCount % 5 == 3) { BotBasicInfoPanel.self.Update() }
        if (this.frameCount % 5 == 0) { BotstatusPanel.SetValues(bot.mode) }
    }
}