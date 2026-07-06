/**
 * @param {Number} min 
 * @param {Number} max
 * @returns {Number} случайное целое от min до max включительно
 */
const RandInt = function (min, max) {
    return min + Math.floor(Math.random() * (max + 1 - min))
}

/**
 * @returns случайный цвет формата #xxxxxx
 */
const RandColor = function () {
    return `#${Math.floor(Math.random() * 16777215).toString(16)}`;
}

/**
 * @returns случайный цвет формата #xxx
 */
const RandColor16 = function () {
    return `#${Math.floor(Math.random() * 4095).toString(16)}`;
}

function Lerp(ta, tb, amt) {
    if (ta > 270 && ta < 360 && tb == 90) { tb = 450 }
    if (ta > 359 && tb == 90) { ta = 0 }

    if (tb == 0 && ta >= 169) { tb = 360 }
    if (ta >= 359) { if (tb == 0 || tb == 360) { return 0 } }

    if (tb == 270 && ta < 90) { tb = -90 }
    if (ta < 0) { if (tb == 270 || tb == -90) { return 360 - ta } }

    return (1 - amt) * ta + amt * tb;
};

class Vector2d {
    /**@private @type {Number}*/ _x;
    /**@private @type {Number}*/ _y;
    /** 
     * @param {?Number|Vector2d} x
     * @param {?Number} y
     */
    constructor(x, y) {
        switch (arguments.length) {
            case 2: 
                this._x = x;
                this._y = y;
                break;
            case 1:
                this._x = x._x;
                this._y = x._y;
                break;
            case 0:
                this._y = this._x = 0;
                break;
            default:
                console.error("Неожиданное количество параметров для конструктора вектора", arguments);
                break;
        }
        //this.scale = scale;
    }
    get x() { return this._x }
    get y() { return this._y }
    set x(v) { this._x = v }
    set y(v) { this._y = v }
    get x_int() { return Math.floor(this._x) }
    get y_int() { return Math.floor(this._y) }

    //get x_scaled() { return Math.floor(this._x * this.scale) }
    //get y_scaled() { return Math.floor(this._y * this.scale) }
    get x_mantice() {
        let xx = this._x < 0 ? 1 + (this._x % 1) : this._x % 1;
        return xx == 1 ? 0 : xx;
    }
    get y_mantice() {
        let yy = this._y < 0 ? 1 + (this._y % 1) : this._y % 1;
        return yy == 1 ? 0 : yy;
    }
    /**
     * @param {Vector2d} from 
     */
    CopyV2(from) {
        this._x = from._x;
        this._y = from._y;
        return this;
    }

    /**
     * @param {Number} x 
     * @param {Number} y 
     */
    Set(x, y) {
        this._x = x;
        this._y = y;
    }

    /**
     * @param {Number} x 
     * @param {Number} y 
     */
    Add(x, y) {
        this._x += x;
        this._y += y;
    }

    /**
     * @param {Vector2d} v 
     */
    Addv2(v) {
        this._x += v._x;
        this._y += v._y;
    }

    /**
     * @param {Vector2d} withV2 
     */
    IsEqually(withV2){
        return (this._x == withV2._x) && (this._y == withV2._y)
    }

    /**
     * @param {Vector2d} target
     * @param {number} amt
     * @param {Vector2d} [translation]
     * @param {number} [accuracy=0.0005] 
     */
    Lerp2d(target, amt, translation, accuracy = 0.0005) {
        if (translation) {
            this._x = Math.abs(this._x - (target._x - translation._x)) > accuracy ? (1 - amt) * this._x + amt * (target.x - translation._x) : target._x - translation._x;
            this._y = Math.abs(this._y - (target._y - translation._y)) > accuracy ? (1 - amt) * this._y + amt * (target.y - translation._y) : target._y - translation._y;
        }
        else {
            this._x = Math.abs(this._x - target._x) > accuracy ? (1 - amt) * this._x + amt * target._x : target._x;
            this._y = Math.abs(this._y - target._y) > accuracy ? (1 - amt) * this._y + amt * target._y : target._y;
        }
    }

    /**
     * @param {Vector2d} target 
     */
    Distance(target) {
        return Math.sqrt(((target._x - this._x) ** 2) + ((target._y - this._y) ** 2));
    }

    /**
     * @param {Number} tx 
     * @param {Number} ty 
     */
    Distance2(tx,ty) {
        return Math.sqrt(((tx - this._x) ** 2) + ((ty - this._y) ** 2));
    }

    toString(){
        return `{x:${this._x},y:${this._y}}`;
    }
}

class MoneyConverter {
    /**
     * 
     * @param {Number|bigint} number
     */
    static toKKKformat(number, nochangeminimum = 100000) {
        if (typeof number != "bigint") { number = BigInt(number) }
        let result = "";
        if (number >= nochangeminimum) {
            if (number <= 1_000_000n && number > 1_000n) {
                return (number / 1_000n) + "K";
            }
            else if (number <= 1_000_000_000n && number > 1_000_000n) {
                return (number / 1_000_000n) + "M";
            }
            else if (number <= 1_000_000_000_000n && number > 1_000_000_000n) {
                return (number / 1_000_000_000n) + "B";
            }
            else if (number <= 1_000_000_000_000_000n && number > 1_000_000_000_000n) {
                return (number / 1_000_000_000_000n) + "T";
            }
            else if (number <= 1_000_000_000_000_000_000n && number > 1_000_000_000_000_000n) {
                return (number / 1_000_000_000_000_000n) + "Q";
            }
            else if (number <= 1_000_000_000_000_000_000_000n && number > 1_000_000_000_000_000_000n) {
                return (number / 1_000_000_000_000_000_000n) + "KQ";
            }
            else {
                while (number > 1000000000n) {
                    number /= 1000000000n;
                    result += "T"
                }
            }
            return number + result;
        }
        return number.toString();
    }
}