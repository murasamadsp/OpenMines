const ItemCodes = OF({
    "tp": 0,
    "resp": 1,
    "up": 2,
    "market": 3,
    "clan": 4,
    "boom": 5,
    "proto": 6,
    "razr": 7,
    "cred": 8,
    "rem": 9,
    "geo_0": 10,
    "geo_c": 11,
    "geo_r": 12,
    "geo_v": 13,
    "geo_ba": 14,
    "geo_w": 15,
    "geo_b": 16,
    "radar_vul": 17,
    "radar_alive": 18,
    "radar_bobot": 19,
    "tpr": 20,
    "constrbot": 21,
    "bg": 22,
    "zz": 23,
    "craft": 24,
    "booms": 25,
    "gun": 26,
    "gate": 27,
    "dizz": 28,
    "stock": 29,
    "scaner": 30,
    "scills": 31,
    "freeup": 32,
    "mine4": 33,
    "geo_h": 34,
    "poly": 35,
    "nanobot": 36,
    "accu": 37,
    "trans": 38,
    "compr": 39,
    "c190": 40,
    "fed": 41,
    "geo_br": 42,
    "geo_rr": 43,
    "auto": 44,
    "emi": 45,
    "geo_rain": 46,
    "spot": 47,
    "nc": 48,
    "dollar": 49,
    "opp": 50,
});

const ItemNameByCode = new Array(51);
for (const key in ItemCodes) {
    ItemNameByCode[ItemCodes[key]] = key;
}

OF(ItemNameByCode);

/** @type {{itemcode:String,icon:HTMLImageElement,src:String}[]} */
const ItemData = new Array(51);

for (const key in ItemCodes) {
    if (typeof key == "string") {
        let i = ItemCodes[key];
        let image = new Image();
        image.src = `graphics/inventory/icons/${i}.png`;
        ItemData[i] = {
            itemcode: key,
            icon: image,
            src: `graphics/inventory/icons/${i}.png`,
        }
    }
}

console.log(ItemData);
