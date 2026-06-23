const SkillIcons = {
    /**@type {String[]} */
    IcoLocations: new Array(56),
    nullableSkillLocation: `graphics/skills/icons/image_part_0.png`,
    emptySlot: `graphics/skills/icons/slot_empty.png`,
}

for (let i = 0; i < SkillIcons.IcoLocations.length; i++) {
    SkillIcons.IcoLocations[i] = `graphics/skills/icons/image_part_${i + 1}.png`;
}


const UP_GUI_SlotsCoords = OF([
    "top: 4px;left: 31px;",
    "top: 87px;left: 0px;",
    "top: 132px;left: 0px;",
    "top: 222px;left: 33px;",
    "top: 49px;left: 14px;",
    "top: 170px;left: 14px;",
    "top: 108px;left: 33px;",
    "top: 70px;left: 45px;",
    "top: 145px;left: 46px;",
    "top: 112px;left: 75px;",
    "top: 16px;left: 74px;",
    "top: 206px;left: 74px;",
    "top: 6px;left: 356px;",
    "top: 57px;left: 356px;",
    "top: 117px;left: 356px;",
    "top: 158px;left: 351px;",
    "top: 85px;left: 112px;",
    "top: 126px;left: 124px;",
    "top: 41px;left: 137px;",
    "top: 8px;left: 183px;",
    "top: 0px;left: 233px;",
    "top: 2px;left: 300px;",
    "top: 31px;left: 268px;",
    "top: 41px;left: 317px;",
    "top: 60px;left: 240px;",
    "top: 70px;left: 280px;",
    "top: 90px;left: 320px;",
    "top: 130px;left: 310px;",
    "top: 110px;left: 260px;",
    "top: 100px;left: 220px;",
    "top: 110px;left: 180px;",
    "top: 70px;left: 200px;"
])

class GuiSkill{
    skillSlot;
    skillLvl;
    skillIcon;
    /**
     * @param {HTMLElement} skillSlot 
     * @param {HTMLElement} skillLvl 
     * @param {HTMLElement} skillIcon 
     */
    constructor(skillSlot,skillLvl,skillIcon){
        this.skillSlot = skillSlot;
        this.skillLvl = skillLvl;
        this.skillIcon = skillIcon;
    }
}

class GUIUPControlls {
    /**@type {HTMLElement} */ GUIPanel;
    /**@type {HTMLElement} */UP_GUI_visual;
    /**@type {GuiSkill[]} */ SkillHTMLElements;

    set visibility(val){this.GUIPanel.style.visibility = val ? "visible": "hidden"};
    get visibility(){return this.GUIPanel.style.visibility == "visible"};

    constructor(){
        this.GUIPanel = document.getElementById("building_gui_panel");
        this.UP_GUI_visual = document.getElementById("GUI_UP_skill_visual");
        this.SkillHTMLElements = new Array(BotSkillsContainer.MaxSkillsCount);
        
        for (let i = 0; i < BotSkillsContainer.MaxSkillsCount; i++) {
            let skillSlot = document.createElement("div");
            skillSlot.id = `GUI_UP_skill_${i}`;
            skillSlot.className = "GUI_UP_empty_slot";
            skillSlot.style = UP_GUI_SlotsCoords[i];
        
            let skillLvl = document.createElement("div");
            skillLvl.className = "GUI_UP_skill_lvl";
            skillLvl.innerText = RandInt(0,5000);
        
            let skillIcon = new Image();
            skillIcon.src = `${SkillIcons.IcoLocations[i]}`;
        
            skillSlot.append(skillLvl,skillIcon);
            this.UP_GUI_visual.append(skillSlot)
            
            this.SkillHTMLElements[i] = new GuiSkill(skillSlot,skillLvl,skillIcon);
        }
    }
}

const UPGUIController = new GUIUPControlls();
UPGUIController.visibility = false;
