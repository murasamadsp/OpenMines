//let map_panel_container = document.getElementById("map_panel_container");
let map_panel_header = document.getElementById("map_panel_header");

let map_canvas = document.getElementById("map_canvas");

const Map_Ctx = map_canvas.getContext("2d");

let map_coords = document.getElementById("map_coords");
let map_options_zoom = document.getElementById("map_options_zoom");
let map_options_night = document.getElementById("map_options_night");
let map_options_opacity = document.getElementById("map_options_opacity");
let map_options_bots = document.getElementById("map_options_bots");

const MapOptions = {
    zoom: map_options_zoom.value,
    night: map_options_night.checked,
    opacity: map_options_opacity.checked,
    bots: map_options_bots.checked,
}

//console.log(MapOptions);

var MapZoom = 2 ** MapOptions.zoom;

class MapDraw {
    wrenderer; canvas; mainCTX; imageData;
    /**
     * @returns {Array};
     */
    GetFirstBlockCoords() {
        let camCenter = this.wrenderer.GetCameraCenterBlockPos();
        return [camCenter.x - ~~(this.canvas.width >> 1), camCenter.y - ~~(this.canvas.height >> 1)];
    }

    /**
     * @param {WRenderer} wrenderer 
     * @param {HTMLElement} canvas 
     */
    constructor(wrenderer, canvas) {
        this.wrenderer = wrenderer;
        this.canvas = canvas;
        this.mainCTX = canvas.getContext("2d");
        this.imageData = new ImageData(this.canvas.width, this.canvas.height);
    }

    Update() {
        //TODO возможность двигать карту мышкой
        this.mainCTX.fillStyle = "#f00";
        this.mainCTX.strokeStyle = "#f00";
        if (this.canvas.width > this.imageData.width || this.canvas.height > this.imageData.height) {
            if (map_panel_container.style.visibility == "")(console.warn("resize"));
            this.imageData = new ImageData(this.canvas.width, this.canvas.height);
            for (let i = 0; i < this.imageData.data.length; i += 4) {
                this.imageData.data[i + 3] = 256;
            }
        }

        let drawPos = this.GetFirstBlockCoords();

        let curcolor;
        let imdataD;
        let dx, dy;
        let colortable = MapOptions.night ? ColorTableDark : ColorTable;
        for (let y = 0; y < ~~(this.canvas.height); y++) {
            for (let x = 0; x < ~~(this.canvas.width); x++) {
                [dx, dy] = [~~(drawPos[0] + x), ~~(drawPos[1] + y)];
                curcolor = colortable[this.wrenderer.WorldMap.GetIDByCoord(dx, dy)];
                imdataD = (y * this.imageData.width + x) << 2;
                [this.imageData.data[imdataD], this.imageData.data[imdataD + 1], this.imageData.data[imdataD + 2]] = [curcolor[0], curcolor[1], curcolor[2]];
            }
        }

        this.mainCTX.putImageData(this.imageData, 0, 0, 0, 0, this.canvas.width, this.canvas.height);

        //TODO вынести в отдельный канвас(там же обработку кликов)
        if (MapOptions.bots) {
            let drawPos2 = [drawPos[0] + this.canvas.width, drawPos[1] + this.canvas.height];
            let bots = this.wrenderer.Bots;
            let botpos;
            for (let i = 0; i < bots.length; i++) {
                botpos = bots[i].position;
                if (botpos.x >= drawPos[0] && botpos.x <= drawPos2[0] && botpos.y >= drawPos[1] && botpos.y <= drawPos2[1]) {
                    //this.mainCTX.rect(botpos.x-1, botpos.y-1, 3, 3);
                    this.mainCTX.fillText("+", botpos.x - drawPos[0], botpos.y - drawPos[1]);
                }
            }
            this.mainCTX.fill();
            this.mainCTX.fillStyle = "#0f0";
            this.mainCTX.fillText("+", this.wrenderer.CurrentBot.position.x - drawPos[0], this.wrenderer.CurrentBot.position.y - drawPos[1]);
            this.mainCTX.fill();
        }
    }
}

let md = new MapDraw(WRender, map_canvas);
//md.Update();


function Mresize() {
    let [cw, ch] = [map_canvas.clientWidth, map_canvas.clientHeight];

    if (map_canvas.width != ~~(map_canvas.clientWidth / MapZoom)) map_canvas.width = ~~(map_canvas.clientWidth / MapZoom);
    if (map_canvas.height != ~~(map_canvas.clientHeight / MapZoom)) map_canvas.height = ~~(map_canvas.clientHeight / MapZoom);
    [map_canvas.style.width, map_canvas.style.height] = [map_canvas.width * MapZoom, map_canvas.height * MapZoom];
}

let pausec = 0;
window.addEventListener("resize", () => {
    // if (map_panel_container.style.visibility == "") {
    //     pausec++;
    //     setTimeout(() => {
    //         pausec--;
    //         if (!pausec) {
    //             Mresize();
    //             md.Update();
    //         }
    //     }, 1000);
    // }
    Mresize();
    md.Update();
}, false);

function MapHasOpened() {
    Mresize();
    md.Update();
}

setInterval(() => {
    if (map_panel_container.style.visibility == "") {
        Mresize();
        md.Update();
    }
}, 300);

map_canvas.addEventListener("mousemove", (event) => {
    let [x, y] = [event.offsetX, event.offsetY];
    let coor = md.GetFirstBlockCoords();
    map_coords.innerText = `Карта ${~~(coor[0] + x / MapZoom)}:${~~(coor[1] + y / MapZoom)}`;
});


map_options_zoom.oninput = (event) => {
    MapZoom = 2 ** event.target.value;
    MapOptions.zoom = event.target.value;
}

map_options_night.onchange = (event) => {
    MapOptions.night = event.target.checked;
}
map_options_opacity.onchange = (event) => {
    MapOptions.opacity = event.target.checked;
    map_panel_container.style.opacity = MapOptions.opacity ? 0.9 : 1;
}
map_options_bots.onchange = (event) => {
    MapOptions.bots = event.target.checked;
}
