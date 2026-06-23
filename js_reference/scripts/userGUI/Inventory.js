class GuiInventoryItemControl {
    #itemBlock; #button; #text;

    get itemBlock() { return this.#itemBlock }
    get button() { return this.#button }
    get text() { return this.#text.innerText }
    set text(value) { if(value != this.#text.innerText){this.#text.innerText = value}}
    /**
     * 
     * @param {HTMLDivElement} itemBlockHTML 
     * @param {HTMLDivElement} buttonHTML 
     * @param {HTMLDivElement} textHTML 
     */
    constructor(itemBlockHTML, buttonHTML, textHTML) {
        this.#itemBlock = itemBlockHTML;
        this.#button = buttonHTML;
        this.#text = textHTML;
    }
}
class GUIInventory {
    isMinimized = true;
    /**
     * 
     * @param {HTMLElement} inventory_container 
     */
    constructor(inventory_container) {
        this.u_inventory = inventory_container.getElementsByClassName("u_inventory")[0];
        this.inventory_syze_button = inventory_container.getElementsByClassName("inventory_syze_button")[0];

        this.inventory_syze_button.value = this.isMinimized ? "<" : ">";
        this.inventory_syze_button.addEventListener('click', (event) => {
            this.SwitchMinimizing();
        })

        /** @type {Map<Number,GuiInventoryItemControl>} */
        this.inventoryCells = new Map();
        /** @type {HTMLDivElement[]} */
        this.inventoryPlaces = new Array();

        for (let i = 0; i < ItemData.length; i++) {
            if (ItemData[i]) {
                let itemBlock = document.createElement("div");
                itemBlock.className = "item";
                let text = document.createElement("div")
                text.className = "item_count";
                text.innerText = i;
                let button = document.createElement("input")
                button.type = "button";
                button.className = "item";
                button.name = `item${i}`

                button.style.backgroundImage = button.style.backgroundImage = `url(${ItemData[i].src})`;


                itemBlock.appendChild(button);
                itemBlock.appendChild(text);
                this.inventoryCells.set(i,
                    new GuiInventoryItemControl(itemBlock, button, text)
                );
            }
        }

        for (let c = 0; c < Math.ceil(ItemData.length / 10); c++) {

            let col = document.createElement("div");// HTMLElement("<div class='column'>")
            col.className = 'column';
            for (let it = 0; it < 10; it++) {
                let itemPlace = document.createElement("div");
                itemPlace.className = "item_place";
                col.appendChild(itemPlace);
                this.inventoryPlaces.push(itemPlace);
            }

            this.u_inventory.appendChild(col);
        }
    }

    SwitchMinimizing(){
        if (this.isMinimized) {
            this.isMinimized = false;
            this.inventory_syze_button.value = ">";
        }
        else {
            this.isMinimized = true;
            this.inventory_syze_button.value = "<";
        }
    }

    SetItemInPlace(itemID, PlaceNum) {
        if (itemID == null) {
            this.inventoryPlaces[PlaceNum].innerHTML &&= '';
        }
        else {
            let invPlace = this.inventoryPlaces[PlaceNum];
            let candidate = this.inventoryCells.get(itemID).itemBlock
            
            if (invPlace.hasChildNodes) {
                if(invPlace.children[0] == candidate){
                    return;
                }
                invPlace.innerHTML = '';
            }
            invPlace.appendChild(candidate);
        }
    }

    /**
     * 
     * @param {Bot} bot 
     */
    Update(bot) {
        if (bot) {
            let iterator = 0;
            bot.inventory.itemsAvailable.forEach((itemCount, itemID) => {
                if (this.isMinimized && iterator > 9) { return }
                this.SetItemInPlace(itemID, iterator);
                this.inventoryCells.get(itemID).text = itemCount;
                this.inventoryPlaces[iterator].style.display &&= "";
                iterator++;
            })
            for (iterator; iterator < this.inventoryPlaces.length; iterator++) {
                this.inventoryPlaces[iterator].innerHTML &&= "";
                this.inventoryPlaces[iterator].style.display ||= "none";
            }
        }
        else {
            for (let i = 0; i < this.inventoryPlaces.length; i++) {
                this.inventoryPlaces[i].innerHTML &&= "";
                this.inventoryPlaces[i].style.display ||= "none";
            }
        }
    }
}