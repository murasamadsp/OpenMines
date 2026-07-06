const WorldCanv = document.getElementById("world_canv");
const canv_container = document.getElementById("canv_container");

const u_inventory_container = document.getElementById("inventory_container");

const WOptions = {
    ignoreBorders: true,
    vectors: false,
    nicknames: true,
    fps: 60,
    progTicks: 10,
    viewBox: false,
}

/** @type {Bot[]} */
const Bots = new Array();

const WC_ctx = WorldCanv.getContext("2d");

window.addEventListener('resize', resizeCanvas, false);
window.onload = () => {
    resizeCanvas();
}

//WC_ctx.clearRect(0, 0, WorldCanv.width, WorldCanv.height);

let wmap = new WorldMap(2, 2);
//wmap.Fill([32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,34,35]);
//wmap.FillRect(35,10,80,200,20);

wmap.LoadWorld(TestWorld0);
wmap.ClearRect(0, 0, 2000, 20);
wmap.ClearRect(1000, 0, 10, 1000);
wmap.ClearRect(200, 0, 10, 1000);
wmap.ClearRect(0, 0, 100, 100);

for (let i = 0; i < 100; i++) {
    wmap.SetTileWithCoords(RandInt(0, 100), RandInt(0, 100), RandInt(0, 1) ? (!RandInt(0, 5)) ? 116 : RandInt(50, 55) : Block.rock_hypno)
}

const WRender = new WRenderer(WC_ctx, wmap);

WRender.Bots.push(new Bot(new Vector2d(50, 50), 2, NickNames.random, WRender));//1154, 100
for (let i = 0; i < 50; i++) {
    let goodcoords = null;
    let [x, y] = [-1, -1];
    while (!goodcoords) {
        x = RandInt(0, wmap.width);
        y = RandInt(0, wmap.height);
        if (BlockStats[wmap.GetIDByCoord(x, y)].hardness >= 0) {
            goodcoords = [x, y];
        }
    }

    let bot = new Bot(new Vector2d(goodcoords[0], goodcoords[1]), 2, NickNames.unique, WRender);
    bot.rotation = RandInt(0, 3) * 90;
    bot.skin = RandInt(0, 4);
    WRender.Bots.push(bot);
}
BotBasicInfoPanel.self.TargetWRenderer = WRender;

const BotsCont = new BotsContainer(wmap.width, wmap.height);
BotsCont.Set(Bots);


var progExecutor = new ProgramExecutor(WRender.Bots);

function ProgTicker() {
    setTimeout(() => {
        if (1000 / WOptions.progTicks < 30) {
            for (let i = 0; i < 50; i++) {
                progExecutor.Execute();
            }
        }
        progExecutor.Execute();
        ProgTicker()
        if (WRender.CurrentBot) {
            if (WRender.CurrentBot.programCondition) {
                let val = WRender.CurrentBot.programCondition.GetState();
                SendBotProgramStatus(val)
            }

        }
    }, 1000 / WOptions.progTicks);

}

ProgTicker();

const guiInventory = new GUIInventory(u_inventory_container);
