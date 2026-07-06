class CryCargo {
    _cry;
    set g(v){v >= 0 ? this._cry[0] = v : console.error("попытка присвоить отрицательный балланс")};
    set b(v){v >= 0 ? this._cry[1] = v : console.error("попытка присвоить отрицательный балланс")};
    set r(v){v >= 0 ? this._cry[2] = v : console.error("попытка присвоить отрицательный балланс")};
    set w(v){v >= 0 ? this._cry[3] = v : console.error("попытка присвоить отрицательный балланс")};
    set v(v){v >= 0 ? this._cry[4] = v : console.error("попытка присвоить отрицательный балланс")};
    set c(v){v >= 0 ? this._cry[5] = v : console.error("попытка присвоить отрицательный балланс")};
    get g(){return this._cry[0]};
    get b(){return this._cry[1]};
    get r(){return this._cry[2]};
    get w(){return this._cry[3]};
    get v(){return this._cry[4]};
    get c(){return this._cry[5]};

    constructor() {
        this._cry = new Uint32Array(6);
    }
}

class PackCargo{
    modules;
    crystalls;
}

class BotCargo extends CryCargo{
    geoLast = -1;
    botstats;
    geo;
    /**
     * @param {BotSkillEffects} botstats
     * */
    constructor(botstats) {
        super();
        this.botstats = botstats;
        this.geo = new Array(botstats.geology);
        for (let i = 0; i < this.geo.length; i++) { this.geo[i] = 0 }
    }

    get geoLastID() { return this.geoLast >= 0 ? this.geo[this.geoLast] : null };
    get geoIsNoFull() { return (this.geoLast + 1 < this.geo.length) }

    get geoFilledPersent() { return ~~((this.geoLast + 1) / this.geo.length) * 100 }
    get geoFilledValue() { return this.geoLast + 1 }


    PopGeo() {
        if (this.geoLast >= 0) {
            let gblock = this.geo[this.geoLast];
            this.geo[this.geoLast] = 0;
            this.geoLast--;
            return gblock;
        }
        return null;
    }

    PushGeo(id) {
        if (this.geoIsNoFull) {
            this.geoLast++;
            this.geo[this.geoLast] = id;
            return true
        }
        return false
    }

    /**
     * @param {"g"|"b"|"r"|"w"|"v"|"c"} key 
     * @returns {Number} коэфициент перевеса по конкретному кристаллу. Если не указать код кри, выдаст максмальное из значений груза
     */
    GetVolume(key = null) {
        if (key) {
            return this[key] / this.botstats.CryCargoSyze(key);
        }
        else {
            //todo вместо говносрача можно просто [].max
            let max = this.GetVolume(CryCodes[0]);
            for (let i = 1; i < CryCodes.length; i++) {
                let crycargo = this.GetVolume(CryCodes[i]);
                if (max < crycargo) { max = crycargo };
            }
            return max;
        }
    }
}

class BotItemInventory{
    
    /** @type {Map<Number,Number>} */ itemsAvailable;

    constructor(){
        this.itemsAvailable = new Map();
    }

    GenerateRandomItems(items = 10,maxCount = 100){
        for (let i = 0; i < items; i++) {
            this.Add(RandInt(0,50),RandInt(0,maxCount));
        }
    }
    
    Check(itemcode){
        return this.itemsAvailable.get(itemcode)?? 0
    }

    Add(itemcode,count){
        let curcount = this.itemsAvailable.get(itemcode)?? 0;
        this.itemsAvailable.set(itemcode,curcount + count);
        return curcount;
    }

    //TODO доработать эту парашу, тут надо чтобы удалялись пустые ячейки ну и нельзя было отнять больше чем есть говна в боте
    /**
     * Взять с инвентаря count штук itemcode. если успешно то вернет текущее количество, если нет то undefined
     * @param {*} itemcode 
     * @param {*} count 
     * @returns {Number|undefined} 
     */
    Take(itemcode,count = 1){
        if(this.itemsAvailable.has(itemcode)){
            let curcount = this.itemsAvailable.get(itemcode);
            if(curcount){
                let result = curcount - count;
                this.itemsAvailable.set(itemcode,result);
                return result;
            }
        }
        return undefined
    }
}
