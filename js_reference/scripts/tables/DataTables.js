const OF = Object.freeze;
const NickNames = {
    base: ["Zapusta", "Максим Елисеев", "Бучик", "Eva Brown", "Кокося", "NFT", "zZer0", "Королева", "E V A", "леня бро леши", "Ferme1", "Misha021", "NikolayBrony", "Flicker", "Vivaldi", "A D A M A N T", "Howbreak", "Teslo", "Xene", "Berserk", "Лень", "Humberf", "Hug", "semKa", "Леша Жерлыгин", "berr", "Подземный робот", "МагисТр", "Sauron", "RAT", "Fake", "Энтин", "Juti", "Евочка", "Skylar", "Kasper", "Karbon Dioksid", "myachin", "D4rL3x", "Omegon", "Yaponchik", "Elza", "Damikol", "JUGGERNAUT", "Канэки", "Alexandr", "БУБОЛЬ", "evee", "kotee", "1", "ПчелК", "Витек", "Dark", "Altew", "Erelzir", "Wiltshire", "X", "jensakaa", "Усмиритель Трикса", "Kirumi", "Игорь Ветров", "блоха", "К И Б У Ч", "Zubko", "CaJLarA", "Металлург", "СОВЕСТЬ", "Gelwar", "LoSt", "Pepega", "Виктория", "Павел Владимирович", "VoRoN", "Poecanus", "VortDyn", "MeNeFrego", "Золотов Саша", "c u r s e d", "xeosn", "Malaya", "шахтолаз", "Кондрашенко", "Andrey XD", "вейдер", "Koza", "ZiiiK", "Leroy", "D M I T R O V", "XuLiGaN", "Илья Шухер", "Nekotama", "will", "Печенька", "Assakura", "whiterabbit", "Fymryn", "Брокер", "Kotovik", "DAP", "Эвелин", "Darkar25", "Evklid", "Roklines", "Kali Yuga", "X", "RoboCop", "MaksRitter", "id133", "Korben Dallas", "Sayfer", "AntiHete", "Igorys", "FPS", "YAMABUSHY", "Мортен", "Фартовий", "Блинчик", "Винил", "DenSenY", "Solanuh", "Анимист", "Dazzai", "X", "Стервятник", "Tuwkan", "Русский", "алекс", "Bender Rodriguez", "Карнидж", "A1pha", "zxc123", "жопа выдры", "seXxYfkdhype2k00swag", "Cocaine", "blessed", "Artem4ik", "Fake", "bfhtfh", "Gen Inferno", "Awesome", "Йцукер", "Zbuk", "Святой", "Дмитрий", "Anazapta", "Почтальон Печкин", "Ckkam", "Пакетик", "Uroplama", "stasik", "San4es", "Firemag", "Апофис", "Mr Reyf", "Pateyrk", "Болт", "001", "fyzel", "ОБ", "мап", "мятный карась", "MAKC163", "LUFI", "dungeon master", "morozilnik", "Дювай", "ящер", "Диваныч", "Космо", "Biba", "Gravinser", "Shaxruz", "kamenschik", "Х Р А Н И Т Е Л Ь", "Nikolai", "kosta", "cedoy", "Айс", "AND178", "Карлеон", "Меченый", "Opium", "Vapone", "Call", "Полосатый мух", "BarBar", "Miss", "Шемонаев", "Darkonis", "Хомяк", "Ксандра", "Umortle", "ReDeissS", "Вади", "Тай Фун", "Warenick", "Граф", "Шани", "JustDragon", "Shemonaew", "Отшельник", "Raindow", "Cotina", "Yappie", "Atlas", "James Bond", "wanderer", "Топер", "Mine Market", "PEPSICS", "k 112", "Chefir", "Диггер", "Калич", "Mafiosy", "Неприподаемый", "DeadLord", "136549", "572", "Flos", "Brair", "SAND KING", "USA", "neMult", "Иван Керченский", "Ржавый Ежик", "Dolly", "Sodon", "4ayka", "Артемий", "PopCorn", "П С И Х", "Hitrour", "Framzi", "Манка", "Ивашка копашка", "o1oOHA", "Cthulhu124i", "B", "Housset", "Даная", "DezQ", "Leonbab", "bionic", "PUSSI CONTROL", "PathFind3r", "Мегагрыз", "амасек", "valy", "Лиловый", "Лагерта", "Phoca", "December", "moralez", "Lexmark", "GOLD2510", "Derzkiy Tapok", "Фрейя", "ЕРОХА", "Фейри", "Рудогрызь", "АЗОВ", "Неудержимый Понос", "Дядюшка", "Усмиритель Винкса", "MrProper", "Де01", "ZAnKi", "Deviant", "Крот", "2", "Evil", "MsLis", "Кусь", "Shpion", "Ветер", "карамультук", "Дилетант", "Nolo", "KoT9lPa", "irkve", "Баск", "Kemius", "Mirtal", "Лапа", "Айтер", "KSEROKS", "BASTET", "VitilitiLaygenX", "Аспирин", "Белый Клык", "K I T S U N E", "YAMABUSHY", "YAMABUSHY", "YAMABUSHY", "vadim136vrn", "Gigabyte", "NIV", "Corleone", "Chezare", "UMkA", "СнуСнумрик", "Лапки добра", "Axeld", "flea", "Хачапури", "Дядя Женя", "Kiks45", "Pork", "Deel", "Akie", "Riffall", "Феникс", "Frit", "Luck", "ЦАЦА", "членосос", "Onyx", "Fake", "МячинХуесос", "AdoIf Hitler", "Yaker", "Unckly", "duracell", "Мармеладка", "Спот", "Vofffka", "COBRA", "PIN", "TakaSHi", "Need", "Aimbotik", "Химик", "Радужный", "Axonor", "Novel", "Littlegnomic", "Миша", "Estriper", "Nbasashok", "Алексей донецк", "хХх", "Big Russian Boss", "Wave", "Kawazaki", "Сергео", "Светлана", "Clown", "Пустотник", "Barbatos", "Kayoshi", "Rick and Morty", "Нафаня", "Sketch", "василиск", "sisimar", "maxd", "КротоОчко", "Nyllon", "Дроб", "Конарик", "RR NEV", "Karl Hanke", "PING", "Синь", "635", "MeRlIn", "дракоша", "ава", "Logeral", "Acro", "пикачу", "Волшебник", "NFS", "Numigua", "Ворлак", "Садист", "nolik", "ruslan", "VAV42", "Kniflut", "Финж", "Sonosavaoff", "Yolo", "DIOR2", "Dath", "Damokls", "tata1072", "375", "Джэк Воробей", "Zerflo", "Jupp", "ЭлитныйЛовкийВолк", "AVE AVOLINAD", "Dona Beija", "Айпери", "Mentl", "Enet", "Pixelix", "Pascarv", "Пипис", "drade", "Легенда", "урсус", "Orioloch", "Theroma", "мастер", "225", "пожинатель", "Jarjaveli", "Хитрый Лис", "ceth", "Пупырь Ежа", "Aikolaf", "Сирена", "GodBless", "Vomora", "Т", "Jerdy", "ROCKET", "Aura", "A", "EvilBoy", "Durnehviir", "A M A R A N T", "спам", "Quent", "Trip", "Котонатор", "хлебушек", "Озимандий", "LUCY YOMO", "Леха", "W", "Fermer", "GENOS", "tak", "Гробовщик", "Jock", "Смеющийся", "Серый", "Samara", "Maestr0", "ПЧЕЛ", "Суетолог", "Землекоп", "Vartes", "Ярь", "iris", "Ywatso", "5551555", "Plaenis", "буличка", "Ash", "NakamuroObossalkus", "Гречка", "Dedge", "Синий", "Великая", "S", "Пашка", "Blata", "VEED", "филя", "Надубий Том", "elua", "DAPik", "дюдю", "Oniopor", "Hombalse", "Skuleaf", "Сон", "Zayo", "Xenoptes", "Дорох224", "Sephae", "Rode", "AcroMan", "Kirain41", "Veskio", "Hedmuth", "Wumfred", "Cera", "Pyramet", "Кибучьзабаньменя", "чи да", "Ptois", "Товарищ Никита", "Данил35", "krisa", "Azel", "Wank", "NIKL ELFAKO", "Ostra", "Huth", "Shadsows", "Мульт", "Adriel", "Jancico", "Cjkchkwrk", "Nickl Elfako", "Vasp", "Ague", "Kaalgrontiid", "Ferdinand Schaal", "Ursua", "Ximiko", "Sexellent", "Почтальон Свечкин", "АбуБандит", "JIOK", "Левиафан", "Guitan", "Tyto", "0 0", "Alduin", "ZloyTapok", "Idel", "0 1", "Bille", "Flower of evil", "Methope", "Nahagliiv", "Xentetra", "Глист", "Aitonix", "Geberne", "Mastof izvini", "Nimeo", "Viinturuth", "Clant", "Vanys", "GriZZLi", "Fatow", "Veret", "Noenick", "Blama", "Vuljotnaak", "Picidea", "000", "Scinelus", "ligion", "Omia", "Евгений", "Gunnan", "Ducky", "STEPAN", "Jaln", "0 2", "Beppiers", "Blaxida", "Леший", "Yakergy", "парапаци", "0 3", "KOTok", "Lucky", "Dyna", "Vulthuryol", "Lobe", "Grilatta", "Sahloknir", "Sostrea", "Kolobok", "Odahviing", "Fireko", "Staniep", "Миледи", "Rlx", "Почтальон Овечкин", "Почттальон Узбечки", "Vail", "Подушка", "Почтальон Узбечкин", "Wolf", "Iscutes", "Jounad", "Implawk", "Aylando", "Frule", "СтРаХЛоЖьСрАтЬВеЧнО", "Shepald", "Ehound", "Мультик", "Леди Бу", "Nylie", "Uheve", "Vinzenz", "Ывавкинсосеух", "zhivotnoe", "додо", "Labirynth", "диди", "IMPERATOR", "Arrow", "zebra", "дада", "Kiat", "Overso", "дядя", "Waig", "Сид Кагэно", "Zaptor", "Мадара Учиха", "Maguid", "Ывавкин", "Whitros", "Ummet", "ImGooD", "Berrnar", "So XIN", "Batyuk", "Krosulhah", "1", "Wolnosc", "Dudetl", "Linolafa", "Gerrene", "Hemiri", "джек мусорщик", "Jeep", "Messing", "Zeng", "Ромашка", "Голодные глазки", "Gazurn", "002", "003", "004", "Darius", "005", "006", "Анютик", "007", "008", "009", "010", "011", "Фенрир", "DaNiL", "ОпарышЪ", "Omega", "Darkonis", "Darkonis", "Darkonis", "Darkonis", "stepan", "Дрема", "Vidal", "Мрак", "SioS", "Чакрум", "Старый", "Spam", "L", "MaxOnXVIII", "Карбозолгидрохлорид", "D", "Rock", "stalone", "Draz", "TIGGER", "Бультерьер", "Dugo", "VinoGRAD", "Fontel", "Алалксей", "Capoloca", "Form", "СветНебес", "Dugosteo", "Odil", "Grizzly", "Стах", "Belmori", "bulochka", "Т", "T34", "Arkatiq", "Dark Hawk", "Ros", "T", "Plereof", "Lover Zubko", "Winx", "Student", "NeMesnii", "Limbo", "Мамочка", "Штольц Диего", "уточка", "Палач Рока", "Ksenon", "Server", "MiniKrot", "Orc", "NEPRIPODAEMYY", "Пельмешка Саурона", "VAK", "RedBerryPony", "Planigon", "Mr", "АЗОН", "Доктор", "kolovrat", "Bounty Hunter", "Yosticen", "R2D2", "дэнчик", "Zircon", "Michamme", "Ginesis", "Emustor", "Sovuh", "pikabu", "XPEHb", "Lyricen", "Seter", "Талос", "Yotrobo", "Vnbn", "Enesse", "Meowch", "Gina", "Tiguanin", "ElF", "mia", "AR21", "Medved", "ArhAngeL", "терминатор", "araverse", "Paymo", "Nabort", "Krolik", "Holl", "СИЛИКОН", "PlRAT", "Стас фембойЧВК", "NightStar", "Леминг", "Linpovsky", "forstr", "сво", "Rampage", "Царевна", "Чушпан", "Blax", "WVALTYRI", "lomka", "Ursin", "Lenok", "SIRIUS", "YAMATA", "Коля перегарро", "The", "ProstoRedt", "Алиса", "Константин", "Aleksey", "Esdrey", "геморройный ежик", "Ходячий Мертвец", "ВаЬу попадают в рай", "Ksidome", "Gech", "Tynel", "Pard", "Wreboala", "Fera", "Nash", "Xanavac", "Фурри фембойчик", "Ventso", "Hombis", "Sagadox", "Смурфик", "Zavod", "Карп", "Zyrsor", "Ummingo", "Fynriq", "Torgall", "Staf", "NPC", "Yosbe", "Leaniork", "Opte", "Ramien", "не кикай заебал", "Poll", "Mirops", "Mutzer", "Studex", "Riness", "Brocton", "Bitne", "Hiro Nimaksi", "Elon", "Euguy", "Aval", "Vomor", "Gale", "Lodo Zarenzo", "Poomes", "Flehoen", "Turos", "Rich Arzider", "Jace", "Laca", "ГраФ", "Qustan", "Oracle", "Розенбаум", "Павел Громов", "Terrorblade", "Vladdos", "Shori", "Wolmost", "Biv", "Earthshaker", "Лариса", "Хвостик", "Avenger2256", "DEN13", "пиявка", "Merunes", "похрен", "Bojack Horseman", "гарри", "Грокс Лидер", "Воин Тьмы", "SAAK EMINA", "Turtlert", "Djonni", "KitCetus", "Zerator", "Neofela", "ubivashka777", "LongDrink", "GOLOVONOG", "DDD", "Gamer2Play", "Odea", "Vaself", "Chilly", "Chero", "Меха", "Tatovka", "Gine", "Lucaprae", "глюк", "Datois", "Buston", "Druby", "Junitch", "Furn", "Logicaw", "Ivestafa", "Rave", "Ivord", "Frostik", "Unkeyend", "Fifti", "Стеклорез", "xDx", "spbronni", "попа", "Rift", "Aizekk", "ананас", "димон", "New", "AsVarna", "Jornaud", "NeoDim", "Терапевт", "Juicy", "MrKiri", "candyhrt", "Сгусток", "UNTLA", "VITS", "Morus", "YAMABUSHY", "Krico", "amigo arkadio", "Trenbolone Acetate", "Olefem", "Сокровенные", "Hippotis", "Folph", "жопа волка", "mirodea", "Noldi", "Silicoid", "Barcyk228", "Mayrot", "S K Y L A R", "Verett", "Vulser", "Zdju Liash", "Gasp Aseriya", "Tome Seda", "Baxtodel", "Drakimon", "Opto", "Irehor", "Etiant", "Krit Osmorus", "Isto Frid", "Asallye", "Acher", "Neeness", "Wyleymaj", "Dioluta", "Verre", "Helin", "Tito Mayd", "Lucian", "Allensey", "Recusta", "Aurleyne", "ХУЙ", "Kuil Fons", "Klet Erzo", "Inest", "Cechton", "Lynaly", "Tabrow", "Culadeny", "Zapis", "Rhase", "Самый злой человек", "Немного злой человек", "Самый добрый человек", "МАСТОФА В РОТ ЕБАЛ", "мастоф хуесос", "ХУЙ БАТИ", "ЗАЛУПА И ЯЙЦА", "NEDAZ", "Frusciante", "Legend", "Дакимакура", "Offline", "Ultraviolence", "The Mother We Share", "Syst", "1080p", "Soldier", "Зараза", "LeXuSS", "Warrior", "Jasm", "Чармилион", "MrMime", "Каспер", "честный", "Лисичка", "залупень пиздатая", "хуйня ебучая", "ТРАХАТЬ", "сын мертвой шлюхи", "уебан", "хуила", "пидорас", "ЗАЛУПА", "Timo Ternan", "Hoby", "Labe Llinrih", "Milopome", "Mirisa", "Patch", "Scota", "Wonflag", "Zinjimmy", "Hitrew", "Uraegata", "Humbeart", "Erki Daymo", "LUCY", "Seague", "Murobsta", "Firilos", "Tela Yoki", "Emmustos", "Crynx", "Coweass", "Ephoer", "Urba Zayogo", "Inna", "Weltonel", "Bingel", "TREGOR", "Jurg", "Kera Kosmas", "Sear", "Mass", "Emus", "Visculosops 222", "Ляля", "А Л Ь Ф А", "Snal", "Pratama", "Squame", "Templaw", "лопата", "Zyrsiot", "Turt", "Quentin", "Tulaptes", "Hulkhil", "Elder Titan", "Triceros", "Otmarc", "Lawave", "Wumed", "NeYro82", "Yvissace", "Ponnis", "Byross", "Doginon", "Digben", "Zenigma", "Speckaxe", "Zapoloch", "Ildmani", "Ailereus", "Sibesi", "Шифоньерная Моль", "Kall", "Jacatfig", "Pollis", "Nickens", "Robroom", "Macyn", "Pome", "Gophryna", "Eptes", "Visa", "Copp", "Chayon", "Inus", "Irops", "Dracidel", "Obozzi", "Egac", "Wiclinus", "Rampa", "Mert Eddjarne", "Zaptus", "Nibu Remen", "Ceralyn", "Mique", "Odem Andob", "Maksido", "Tera", "Dylvaria", "ВАЛЛИ", "Nycetor", "MineXinec", "Маким Блатной", "cool", "черный", "Isospoda", "Ignal", "YEGIR", "NorgeSS", "Zapa", "Seaglem", "Wylly", "Ruidge", "Amethiel", "Salin", "Emyota", "Brix", "Kari", "Axella", "Voln", "Eria", "Vulleana", "Kjeral", "Plafa", "Helmes", "Ebes Sualtito", "Inchus", "Witch", "Peds", "Jasterna", "Waizinn", "Vescrew", "Urien", "Ambe Linal", "Zain", "Zeugo", "Stonel", "Robstle", "Armora", "Wongarid", "Uldeon", "Lasp", "Hilt", "Chae Dizos", "Pyrageo", "Vicerus", "Vari", "Xzaddelo", "Quennell", "Alpha", "Колобок", "Oplance", "Uguy", "Robre", "Jevon", "Xenammus", "Xzair", "Zentel", "Othus", "Obynne", "Simi Ladvit", "Tsezan", "Oricare", "Skro", "Pida", "Mess", "Koatfig", "Malvern", "Frostik1", "Meseshog", "Uggi", "Justin", "Erme Ntayoton", "Hoda", "Maldic", "Iasirel", "Rolter", "Tukiros", "Zefobis", "Quennyon", "Арчи", "Oswen", "Quinnan", "Ajajao", "Broonbat", "Hyster", "Aubila", "Orne", "Zenak", "Yakeyen", "Ummil", "Benum", "Kaloutec", "Ginusz", "Koatfise", "Rhis", "Hech", "Teuta", "Aklou", "Dayo Goito", "Uglidel", "mentalistixxx", "Crussader", "Маркиза", "Ulix", "Triange", "Ruto", "Guidge", "Cebufo", "Mustin", "Wishift", "Ereuta", "Chay Kseus", "Rhistork", "Tjoffens", "Yert", "Midozir", "Nayd Amon", "Murgy", "Metticus", "Hippon", "Light", "Dustar", "Indidel", "Jachain", "Peant", "Juntrost", "Ohyench", "Grus", "Strigo", "Narkien", "Guigino", "Laspao", "Etis", "Guisa", "Ulisto", "Igodjio", "Odal Berhold", "Putost", "Modyto", "Ountl", "Inguina", "Yoha Ydikt", "Sofirdj", "Sens Ente", "Flose", "Ephemega", "Geni Goito", "Vess", "Deush", "Fringbil", "Eppome", "Galahad", "natka", "Kaney", "Zachazia", "Alionkey", "Zapapus", "Stor Tman", "Judel", "Guarana", "Bass", "Quironwe", "Gyperma", "Igatec", "Barclay", "Putze", "Ryplane", "Zapola", "Cola", "Etirel", "Boud", "Urbanuez", "Fele Riskua", "Лирт", "Zagger", "TECHIES hihihi", "MOHAX", "Полиночка", "hectorkipio", "КРИКСАЛИС", "ИРИШКА", "Татьяна Пузенкова", "Irratta", "Odolf", "Tettia", "Binchus", "Joyaloyd", "Delbes", "Trens", "Clausty", "Neenal", "Xendipon", "Masm", "Hrif Oneleus", "Zerflock", "Cotor", "Ruide", "Raphus", "Hata", "Dyammy", "Lyrn", "Jinally", "Grert", "Iljaren", "Klauru", "Vaza Ymos", "Eauroo", "Currante", "Tyce", "True", "Cletorex", "Uwolto", "Ksia Guga", "Munion", "Citech", "Fernd", "Beast", "BOT", "Доктор Вазелин", "Rine", "Нежданчик", "TEHb", "Вурдалак", "Wehrmachtsgeneral", "Ivan Kullier", "Nicitch", "EvilBear", "Kadi Milosh", "РинэБлинВашуЖзаНогу", "ева", "Aleksey12", "vlad", "ШАЙТАН ТРУБА", "изя факер", "Не Григорий Мороз", "Cardinal", "Shankai", "Cyon", "Waspect", "Foryx", "Rudo", "Difly", "Birperm", "ASLAVA TOGEI", "Clay", "Gateloth", "Ratar", "Lizarmag", "Hylogona", "Okapasta", "Campha", "Zavis", "Mornix", "Yustino", "Guna", "Giactato", "Prew", "Wilfe", "R a d i c a l", "Arny", "Глубина", "Любовник", "Патриархия", "korvin", "Елена", "Заднеприводный Робот", "fepivi", "IZATON4IK", "Надюша", "viv", "kisa1243", "Raiden", "Poga", "огневушка", "Злобный", "Friel", "Sevka", "Ouieu", "Prim Ilia", "Hirm Austor", "Chirino", "Chismot", "Murtle", "Sorrest", "Ronsenk", "Chez Aymolo", "Yorkamis", "Solo Naydras", "Palgerre", "Котик", "Dragon", "Chief Keef", "pan1xd", "Алмазик", "Время", "Krizenk", "Kleopo", "Pahrist", "Mirko", "i Its i", "LUI", "Лукка", "Alex", "mahiwa", "Bruno", "АнаЛизатор", "Dunerunner", "Filk Edga", "Dythy", "Hustler", "MrSlendyBoy", "BigDilla", "Zench", "копалкин", "Egab", "Kudesnik", "Kaydon", "Мама", "Nezuko", "yappy", "Ownleryl", "314ZZA", "УтиЛизатор", "Ibir", "Tigross", "Shimil", "внутри меня кровь", "Gred", "Tricawk", "Ciera", "Тысяча чертей", "Full", "Melvy", "Flobat", "Alda Niel", "Сланец", "Analitik", "Zerobat", "Woodrons", "Boatud", "Unkey", "Svell", "Clockind", "Igur", "Culbud", "Buffeaf", "Fass Elis", "Famill", "Uanuses", "Yann Ermokloud", "Ruemo", "Guan", "Beack", "Neytavo", "Zeod Riker", "Vustar", "Plagel", "Franist", "Xano", "Phia", "Latra", "Chiron", "Dast", "Yust Echaymo", "Ushanse", "Gratud", "Hiro Niordjel", "Chincota", "Eptaiana", "Dalius", "Endel", "Aythel", "Byndo", "Blas", "Meles", "Ethane", "алик", "лиана", "марта", "дима", "ваня", "Шао", "ПИРАТ", "Lissus", "Nest Reto", "линка", "сивка бурка", "валя", "3612", "alex", "Zade", "джон уик", "Ecanumia", "Vally", "Gholeth", "Ktis Edjisedj", "Psitorex", "Dusto", "Erhel", "Ushant", "Azur", "Byne", "Talpiifo", "Vesseart", "Jine", "Fran Insen", "Verdd", "Tustrix", "Forus", "Zaini", "Mohamlye", "Whiplane", "Epheno", "евгений1", "антон", "TwI3Y", "альбина", "надежда", "гиолин", "алех", "грег", "аня", "BloodVizer", "Overlord", "Оливочка", "Vlaz Aruseous", "Halesh", "Jadicarf", "Белка Кемерово", "тулька", "Эндик", "соломия", "Ирина", "уув", "Валентин", "Прометей", "Despair", "рустам", "Iven", "Conifo", "Yakert", "BbIXYXOLb", "ддд", "Володя", "Клара", "Пойзон", "Ксюша", "у тебя", "Александр", "Пудж", "Dneven", "rgb", "Poldgavi", "Анна", "Вадим", "Кирилл", "егор", "владимир", "Ваня1", "Ging", "Rineepha", "бот", "Mandri", "Imigfrot", "Caso", "Etseld", "Somense", "Terrene", "Meillens", "Igalus", "Jadergy", "MushNoise", "Bond Eberkt", "Arima Kishou", "Spiko", "Rhida", "Tynnodes", "casic", "Maksim", "Didea", "Sabi Odelis", "Куркума", "Chechevichka", "Butlerf", "Xaniel", "Maks Gorov", "Lawk", "Iros Teyrk", "Sofidon", "renocki", "Iramir", "Dravin", "Ликава", "Izeubero", "Chudak", "Damidar", "Etissacq", "Narindo", "Lutz", "Rupialle", "Owley", "Humottem", "TheEnderson", "Voma", "Domi Losh", "Noacoby", "Klinzir", "Myotis", "defstroyk", "Exploratore", "Oneo", "Kniffea", "Twern", "Admill", "Visse", "Figer", "Pati Sindrob", "BuzzCoin", "Iger", "Grew", "Yake", "Wendale", "Diedjet", "ZloyKot", "Mark", "Сяо", "ENEM", "chukcha", "DinaAngelFox", "Dune222", "Azurn", "Boal", "Buttec", "Picho", "Igops", "Whexpeng", "Wasm", "Arno Yuzk", "Threa", "paradoksMachonki", "Serinat", "камилла", "Алекс1", "Лиза", "Гена", "катя", "MastofBIck", "sevin", "Миша 1", "дима 1", "артур", "ЯСЯ", "бажигма", "Shievent", "дина", "лиса", "говард", "димка", "Knuszlor", "Pito Sval", "Htof Oroaneys", "Продам кк за реал", "Quirian", "DrAmm", "Gabor", "Foroung", "Umesor", "Alcesier", "Ghorn", "Drakosha", "Ship", "Smidias", "RitterHyitter", "Продам бумы по", "Селектор битов", "Grenda", "Hitomi", "BeHappy", "Удержимая подлива", "хПх", "Luge", "Andrey", "Sirii", "Pont", "Larisa", "Sciterna", "Limia", "Biumamus", "Vomecus", "Kjehons", "Yves", "Hordal", "Kelly", "Otto", "Eboshoc", "Yoha Stomandr", "Tabre", "Dyttho", "Shiflag", "Uropla", "Zeod Origen", "Tight", "GizmoKotizmO", "Мега Пицца", "Belladonna", "Vallis", "Lamp Reht", "Dift", "Uroporus", "Barorda", "Z", "Plalt", "Vois Mannino", "Framon", "Florin", "Kuros", "Tonayd", "Kandlem", "Feavense", "Igata", "Хуйлан", "Вам помочь а", "Tokiva", "Йегерь", "Иди нахуй", "Пизда", "Spens", "Heion", "Raver", "ввв", "Swen", "Zenape", "Davo", "Smias", "Adis", "Azurame", "Dylar", "Эна", "Вио", "Триа", "Все мои раны", "Rex", "VAVILON", "Hinnel", "Iprima", "Cnaimiri", "Dobr Ondeliash", "Hesmiri", "Plewlie", "Myrnix", "Bracidea", "Quiriq", "Thitch", "Jahimaj", "Userpet", "Opark", "Ivorane", "Char Aleonid", "Елена1", "Dren Zaroldeo", "М И Л Ф А", "зшкпуп", "Наталья", "дениска", "мии", "Thror", "Zermos", "Nifene", "Barbie", "Grebriel", "Zenerver", "Rudjero", "Yvinz", "сер гэй", "Bitra", "рысь", "Николай", "Натали", "DiDlliK", "айс1", "Blobat", "Iroda", "ывавкин легенда", "Ариэль", "ALBLAK52", "FRESCO", "CavEmpt", "LIL JEEP", "Yojhi Yamamoto", "Пачтальон Гречкин", "Ziggs", "манул", "тигр", "пума", "сервал", "Xx", "Ru", "Bliliurdontha", "Kmermi", "Broke", "роз", "Spel Leus", "Meec", "sergo", "Tsez Idel", "Darvoln", "d1gger nigger", "Vaseness", "Дикая", "Orion", "Кибер", "Litadyte", "Bristleback", "Точечка", "Valera", "Нариман Фелемузов", "Bitrost", "NyChiKruL", "Coolco555", "враг народа", "Kinokipa", "SPECTRE", "Dimus", "Edward", "Kreouds", "Ведьма", "Mouse", "Loxodida", "Tedrian", "Renessal", "Rhino", "Fitchase", "Planey", "Limus", "Humocelf", "Makc Hintielo", "Бро", "Икорка", "цан", "Peppie", "Haega", "Greb Olen", "Braint", "Estela", "Igora", "Sigmart", "Fuldexpe", "Wyndork", "Valkyrie", "Oriano", "Mnanes", "Gazel", "NLE CHOPA", "unc909", "LaserBot", "ime", "Umork", "Yohd Arik", "Jova", "Patab", "Welderl", "Frosmind", "Twea", "Viveria", "Frene", "Karlmar", "Goatch", "Xentopus", "Asculuca", "Yoha Ymund", "Torch", "Ilve Ssanto", "Dunslo", "Reama", "Rodovid", "Zaing", "Zintis", "Dragonborn", "мозготрах", "Jelm Irgirono", "Azazello", "AZAZLO", "BABY MELO", "POIZON", "Gruine", "Lil Uzi Vert", "Lil Pump", "Lil Yachty", "Spir", "Lyose", "Hylobri", "Junthone", "Wreboark", "Uayd Ayon", "Kosh", "Chustan", "Durna", "Seterio", "Tynnido", "Ortius", "Solagon", "Lizaro", "SANS27", "STO PROTC NE DAZ", "Hyadgett", "Ulva Lenio", "MagneC", "ПиСюГан", "Рахат Лукум", "Ami56", "нью Топер", "Xaron", "Ункля", "FogUser", "UP I", "жопа жопы выдры", "Kizaru", "Меркурий", "kirirom", "Alastor", "Aymon", "Nero Uaydemos", "Shim Ireydk", "margo", "Martini", "Crystard", "Viliar", "Balse", "laymik", "гепард", "4otis", "Любимой", "Лис", "NZT", "Dandy666", "Solbero", "one", "Райан Гослинг", "Патрик Бэйтмен", "Valafar", "sadam", "mashenka666", "Анна109", "Si3Pio", "kooba", "Ratta", "4YDnO1", "Nax", "LorD", "Abram", "Kirill5140K", "Skywalker", "Murasama", "Erecht", "Nimona", "Рагдай", "Thozoa", "Kamin", "RedPanda", "MrMafos", "Prime", "Fonsondo", "metal", "Yampenro", "PahaKakaha", "Ultimatia", "Маслинка", "Gojo Satoru", "богдан", "BazMoody", "Rupius", "MrrPilligrim", "Celo", "Duvon", "Koreyaski", "Cristal", "Atra", "Бу Кэт", "Chardi", "Pelepis", "abaputin", "JIEPMOHTOB", "evgen", "Wartoo", "filambora", "Tuscoma", "Черепах", "Luci", "Faposlav", "Igale", "мракобес", "Carro", "WARGOTS", "Молли", "Irokese71", "Хуетень", "Катти Рей", "Arbuzerka", "Hilli", "Dawn", "Бармолей", "Harv", "Chert", "admin kurwa", "zik3D", "DreamAngel", "Сутенер", "Kipie", "Avgu Stan", "WenXDY", "Stela", "жопа шемонаева", "Flamide", "робот пылесос 3000", "d0ps1g", "Sques", "Saak Seydi", "Lika Noruset", "someone", "Nemunit", "DeadLock", "you are idiot", "Андрес351", "Teontix", "Медвежонок", "ushweak", "пидарас", "Vazayot", "CorRaven", "NikolayDDD", "SBHolder", "L u d u m", "SunkenBee", "Yar4ik", "Копатыч", "п а р а н о й я", "C R O W N", "света шлюха", "Vainie", "Vitay", "Flaguid", "gavarunchuk", "HeKeT", "Фсемирное зло", "Фся такая в белом", "Zubko 2", "Krotozemlekop", "SHADOW DEMON", "писька", "Vomys", "Аро", "Ponkey", "Lycteo", "Vasey", "Aset", "Куави", "Древнее Зло", "Tea", "stolik", "Lestbe", "Plagum", "Podpivas", "Dravius", "Величайший фермер", "Saguini", "Мыш", "GUTS", "ViP", "ksandr900", "Архангел", "ЧЕГЕРМАН", "Templar Assasin", "Owlaf", "Македа", "200VAT", "ZLOI", "Feodall", "RED", "Volkov", "SergeLeon", "Assripper", "tiom4eg", "Strelka", "Atidox", "svyat", "sleepwalker", "Истина", "Miner9000", "Curichla", "Бог", "Zerver", "Magistrix", "Seeing the dawn", "каменьщик", "Шаги", "Конец", "sasori", "DETESH", "Serksim", "ReFLeX", "Enver17", "Dwaylor", "Knoa", "Гриб", "Juliyadraw", "DarknessJudge", "DARK LORD", "ТихиеШаги", "нуам", "ДенДик", "Shakhter", "Wrien", "Artonst", "ТЕНЕВАЯ ИЩЕЙКА АЛАРД", "Fredge", "TOPER777", "Led", "vois3586", "BloodAlloy", "Iwarmo", "Грарри Поттер", "Rupio", "Blattus", "Qwest", "siri", "ЛисЛис", "acheronicc", "bandugan", "троль001", "Jenarka", "Crazydog", "Leon", "Alex", "Миомио", "Omkey", "Kalphond", "Peaspech", "Fanis", "Kevin", "Foxi", "Kotikmeow11", "Kalt", "вавил", "Hatura", "UraFeed", "Clemus", "Narola", "Snolof", "kApateJlb", "Roma Ndelipp", "narola2", "Tifun", "zloyZerg", "Agel"],

    /**
     * @returns {String}
     */
    get random() {
        let i = Math.floor(Math.random() * this.base.length);
        let newnick = this.base[i];
        if (typeof (newnick) == "string") {
            this.base[i] = { counter: 0, name: newnick };
        }
        newnick = this.base[i];

        if (newnick.counter == 0) {
            newnick.counter++;
            return newnick.name;
        }
        newnick.counter++;
        return newnick.name + newnick.counter++
    },
    /**
     * @returns {String}
     */
    get unique() {

        for (let i = 0; i < this.base.length; i++) {
            let newnick = this.base[i];
            if (typeof (newnick) == "string") {
                this.base[i] = { counter: 1, name: newnick };
                return newnick;
            }
        }
        return this.random;
    },
}

const nameByIDtable = new Uint8Array(256);

nameByIDtable[0] = "none0";
nameByIDtable[1] = "none1";

for (let i = 2; i < 256; i++) {
    nameByIDtable[i] = "none" + i;//////
}
nameByIDtable[30] = "ворота";//
nameByIDtable[31] = "вулкан";//
nameByIDtable[32] = "земля";//GROUND0
nameByIDtable[33] = "земля1";//GROUND1
nameByIDtable[34] = "земля2";//GROUND2
nameByIDtable[35] = "дорога";
nameByIDtable[36] = "з-покрытие";
nameByIDtable[37] = "вход в здание";
nameByIDtable[38] = "угол здания";
nameByIDtable[39] = "полимер";
nameByIDtable[40] = "черный валун (о)";
nameByIDtable[41] = "черный валун (к1)";
nameByIDtable[42] = "черный валун (к2)";
nameByIDtable[43] = "мет. валун (к)";
nameByIDtable[44] = "мет. валун (с)";
nameByIDtable[45] = "мет. валун (ф)";
nameByIDtable[48] = "квадро";
nameByIDtable[49] = "опора";
nameByIDtable[50] = "голубая жива";
nameByIDtable[51] = "красная жива";
nameByIDtable[52] = "фиол жива";
nameByIDtable[53] = "черная жива";
nameByIDtable[54] = "белая жива";
nameByIDtable[55] = "радужная жива";
nameByIDtable[60] = "белый песок (с)";
nameByIDtable[61] = "белый песок (т)";
nameByIDtable[62] = "красный песок (с)";
nameByIDtable[63] = "красный песок (т)";
nameByIDtable[64] = "серый песок (с)";
nameByIDtable[65] = "серый песок (т)";
nameByIDtable[66] = "бел слизь";
nameByIDtable[67] = "фиол слизь";
nameByIDtable[68] = "перл";
nameByIDtable[71] = "зел х4";
nameByIDtable[72] = "син х3";
nameByIDtable[73] = "крась х2";
nameByIDtable[74] = "голь х2";
nameByIDtable[75] = "фиол х2";
nameByIDtable[80] = "ВБ констр";//
nameByIDtable[81] = "ВБ";//ВБ
nameByIDtable[82] = "ВБ слизь";//
nameByIDtable[86] = "желтая слизь";
nameByIDtable[90] = "бокс";
nameByIDtable[91] = "магма";
nameByIDtable[92] = "серый валун (ср)";
nameByIDtable[93] = "серый валун (темн)";
nameByIDtable[94] = "серый валун (св)";
nameByIDtable[95] = "none95";
nameByIDtable[96] = "none96";
nameByIDtable[97] = "синий песок (т)";
nameByIDtable[98] = "синий песок (с)";
nameByIDtable[99] = "желтый песок (т)";
nameByIDtable[100] = "желтый песок (с)";
nameByIDtable[101] = "зел. блок";
nameByIDtable[102] = "желт. блок";
nameByIDtable[103] = "фиол скала";
nameByIDtable[104] = "фед-блок";
nameByIDtable[105] = "красн. блок";
nameByIDtable[106] = "фрейм здания";//здания
nameByIDtable[107] = "зел кри";
nameByIDtable[108] = "крас кри";
nameByIDtable[109] = "син кри";
nameByIDtable[110] = "фиол кри";
nameByIDtable[111] = "бель кри";
nameByIDtable[112] = "голь кри";
nameByIDtable[113] = "синий златоскал";
nameByIDtable[114] = "черноскал";
nameByIDtable[115] = "черноскал зеленый";
nameByIDtable[116] = "синяя жива";
nameByIDtable[117] = "красноскал";
nameByIDtable[118] = "кислотка";
nameByIDtable[119] = "гипноскал";
nameByIDtable[120] = "златоскал";
nameByIDtable[121] = "зеленая скала";
nameByIDtable[122] = "говноскал";

class BlockStatistics {
    hasdensity = false;
    hardness = -1;
    replesable = false;
    cantake = false;
    /** было type.block */
    solid = false;
    /**@type {0|1|null} определение в FallType*/
    falltype = null;
    diggablerock = false;
    damage_fall = 0;
    damage_digg = 0;
    is_alive = false;
    is_slime = false;
    is_cry = false;

    /**
     * @typedef {{ hasdensity?:Boolean, hardness?:Number,replesable?:Boolean,cantake?:Boolean, solid?:Boolean, falltype?:0|1,diggablerock?:Boolean,is_alive?:Boolean,is_slime?:Boolean,is_cry?:Boolean, damage_fall?:Number,  damage_digg?:Number}} BlockStatisticsParams 
    */

    /** @param {BlockStatisticsParams} params */
    constructor(params) {
        this.SetParams(params)
    }

    /** @param {BlockStatisticsParams} params */
    SetParams(params) {
        for (const key in params) {
            if (Object.hasOwnProperty.call(this, key)) {
                this[key] = params[key];
            } else {
                console.warn("хз что за параметр", key, params);
            }
        }
    }
}



/** @type {BlockStatistics[]} */
const BlockStats = new Array(256);
const basicBS = new BlockStatistics()//OF({ hasdensity: false, hardness: -1, type: { replesable: false, cantake: false, block: false, falltype: null }, damage: { fall: 0, digg: 0 }, })
for (let i = 0; i < BlockStats.length; i++) {
    BlockStats[i] = basicBS;
}

const FallType = OF({
    sand: 0,
    bolder: 1,
})


BlockStats[30] = new BlockStatistics();// "ворота";//
BlockStats[31] = new BlockStatistics();// "вулкан";//
BlockStats[32] = new BlockStatistics({ replesable: true });// "земля";//GROUND0
BlockStats[33] = new BlockStatistics({ replesable: true });// "земля1";//GROUND1
BlockStats[34] = new BlockStatistics({ replesable: true });// "земля2";//GROUND2
BlockStats[35] = new BlockStatistics({ replesable: true });// "дорога";
BlockStats[36] = new BlockStatistics();// "з-покрытие";
BlockStats[37] = new BlockStatistics();// "вход в здание";
BlockStats[38] = new BlockStatistics({ solid: true, damage_fall: 1000 });// "угол здания";
BlockStats[39] = new BlockStatistics();// "полимер";
BlockStats[40] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 100 });// "черный валун (о)";
BlockStats[41] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 100 });// "черный валун (к1)";
BlockStats[42] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 100 });// "черный валун (к2)";
BlockStats[43] = new BlockStatistics({ hardness: 4191, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 200 });// "мет. валун (к)";
BlockStats[44] = new BlockStatistics({ hardness: 4191, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 200 });// "мет. валун (с)";
BlockStats[45] = new BlockStatistics({ hardness: 4191, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 200 });// "мет. валун (ф)";
BlockStats[48] = new BlockStatistics({ hasdensity: true, hardness: 741, solid: true, damage_fall: 10 });// "квадро блок";
BlockStats[49] = new BlockStatistics({ hasdensity: false, hardness: 0, solid: true, damage_fall: 1 });// "блок опора";
BlockStats[50] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, is_alive: true, damage_fall: 200 });// "голубая жива";
BlockStats[51] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, is_alive: true, damage_fall: 100 });// "красная жива";
BlockStats[52] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, is_alive: true, damage_fall: 200 });// "фиол жива";
BlockStats[53] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, is_alive: true, damage_fall: 200 });// "черная жива";
BlockStats[54] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, is_alive: true, damage_fall: 100 });// "белая жива";
BlockStats[55] = new BlockStatistics({ hardness: 2091, cantake: true, solid: true, is_alive: true, damage_fall: 300 });// "радужная жива";
BlockStats[60] = new BlockStatistics({ hardness: 201, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 10 });// "белый песок (с)";
BlockStats[61] = new BlockStatistics({ hardness: 201, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 10 });// "белый песок (т)";
BlockStats[62] = new BlockStatistics({ hardness: 411, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 15 });// "красный песок (с)";
BlockStats[63] = new BlockStatistics({ hardness: 411, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 15 });// "красный песок (т)";
BlockStats[64] = new BlockStatistics({ hardness: 831, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 17 });// "серый песок (с)";
BlockStats[65] = new BlockStatistics({ hardness: 831, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 17 });// "серый песок (т)";
BlockStats[66] = new BlockStatistics({ hardness: 152, solid: true, falltype: FallType.sand, is_slime: true, damage_fall: 100, damage_digg: 50 });// "бел слизь";
BlockStats[67] = new BlockStatistics({ hardness: 341, solid: true, falltype: FallType.sand, is_slime: true, damage_fall: 200, damage_digg: 200 });// "фиол слизь";
BlockStats[68] = new BlockStatistics({ hardness: 341, solid: true, falltype: FallType.sand, is_slime: true, damage_fall: 350 });// "перл";
BlockStats[71] = new BlockStatistics({ hasdensity: true, hardness: 96, cantake: true, solid: true, is_cry: true, damage_fall: 100 });// "зел х4";
BlockStats[72] = new BlockStatistics({ hasdensity: true, hardness: 131, cantake: true, solid: true, is_cry: true, damage_fall: 100 });// "син х3";
BlockStats[73] = new BlockStatistics({ hasdensity: true, hardness: 201, cantake: true, solid: true, is_cry: true, damage_fall: 100 });// "крась х2";
BlockStats[74] = new BlockStatistics({ hasdensity: true, hardness: 411, cantake: true, solid: true, is_cry: true, damage_fall: 100 });// "голь х2";
BlockStats[75] = new BlockStatistics({ hasdensity: true, hardness: 224, cantake: true, solid: true, is_cry: true, damage_fall: 100 });// "фиол х2";
BlockStats[80] = new BlockStatistics({ solid: true, damage_fall: 10, damage_digg: 2 });// "ВБ констр";//
BlockStats[81] = new BlockStatistics({ hasdensity: true, hardness: 66, solid: true, damage_digg: 20, damage_fall: 100 });// "ВБ";
BlockStats[82] = new BlockStatistics({ hardness: 66, cantake: true, solid: true, falltype: FallType.sand, is_slime: true, damage_digg: 1, damage_fall: 20 });// "ВБ слизь";//
BlockStats[86] = new BlockStatistics({ hardness: 68, solid: true, falltype: FallType.sand, is_slime: true, damage_digg: 10, damage_fall: 20 });// "желтая слизь";
BlockStats[90] = new BlockStatistics({ solid: true });// "бокс";
BlockStats[91] = new BlockStatistics({ hardness: 43, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 100 });// "магма";
BlockStats[92] = new BlockStatistics({ hardness: 1041, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 110 });// "серый валун (ср)";
BlockStats[93] = new BlockStatistics({ hardness: 1041, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 110 });// "серый валун (темн ";
BlockStats[94] = new BlockStatistics({ hardness: 1041, cantake: true, solid: true, falltype: FallType.bolder, damage_fall: 110 });// "серый валун (св)";
BlockStats[97] = new BlockStatistics({ hardness: 96, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 10 });// "синий песок (т)";
BlockStats[98] = new BlockStatistics({ hardness: 96, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 10 });// "синий песок (с)";
BlockStats[99] = new BlockStatistics({ hardness: 43, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 2 });// "желтый песок (т)";
BlockStats[100] = new BlockStatistics({ hardness: 43, replesable: true, cantake: true, solid: true, falltype: FallType.sand, damage_fall: 2 });//= "желтый песок (с)";
BlockStats[101] = new BlockStatistics({ hasdensity: true, hardness: 66, solid: true, damage_fall: 10 });//= "зел. блок";
BlockStats[102] = new BlockStatistics({ hasdensity: true, hardness: 91, solid: true, damage_fall: 10 });//= "желт. блок";
BlockStats[103] = new BlockStatistics({ hardness: 43, solid: true, diggablerock: true, damage_fall: 10 });//= "фиол скала";
BlockStats[104] = new BlockStatistics({ solid: true, damage_fall: 10000 });//= "фед-блок";
BlockStats[105] = new BlockStatistics({ hasdensity: true, hardness: 391, solid: true, damage_fall: 10 });//= "красн. блок";
BlockStats[106] = new BlockStatistics({ solid: true, damage_fall: 10000 });//= "фрейм здания";
BlockStats[107] = new BlockStatistics({ hasdensity: true, hardness: 14, cantake: true, solid: true, is_cry: true, damage_fall: 50 });//= "зел кри";
BlockStats[108] = new BlockStatistics({ hasdensity: true, hardness: 96, cantake: true, solid: true, is_cry: true, damage_fall: 50 });//= "крас кри";
BlockStats[109] = new BlockStatistics({ hasdensity: true, hardness: 61, cantake: true, solid: true, is_cry: true, damage_fall: 50 });//= "син кри";
BlockStats[110] = new BlockStatistics({ hasdensity: true, hardness: 61, cantake: true, solid: true, is_cry: true, damage_fall: 50 });//= "фиол кри";
BlockStats[111] = new BlockStatistics({ hasdensity: true, hardness: 61, cantake: true, solid: true, is_cry: true, damage_fall: 50 });//= "бель кри";
BlockStats[112] = new BlockStatistics({ hasdensity: true, hardness: 61, cantake: true, solid: true, is_cry: true, damage_fall: 50 });//= "голь кри";
BlockStats[113] = new BlockStatistics({ hardness: 306, solid: true, diggablerock: true, damage_fall: 10 });//"синий златоскал";
BlockStats[114] = new BlockStatistics({ solid: true, damage_fall: 500 });//= "черноскал";
BlockStats[115] = new BlockStatistics({ solid: true, damage_fall: 500 });//= "черноскал зеленый";
BlockStats[116] = new BlockStatistics({ hardness: 1041, cantake: true, solid: true, is_alive: true, damage_fall: 50 });//= "синяя жива";
BlockStats[117] = new BlockStatistics({ solid: true, damage_fall: 1000 });//= "красноскал";
BlockStats[118] = new BlockStatistics({ hardness: 516, solid: true, replesable: false, diggablerock: true, is_slime: true, damage_fall: 10, damage_digg: 5 });//= "кислотная скала";
BlockStats[119] = new BlockStatistics({ solid: true, cantake: true, damage_fall: 500 });//= "гипноскал";
BlockStats[120] = new BlockStatistics({ hardness: 1881, solid: true, diggablerock: true, damage_fall: 50 });//"златоскал";
BlockStats[121] = new BlockStatistics({ hardness: 3666, solid: true, diggablerock: true, damage_fall: 70 });//"зеленая скала";
BlockStats[122] = new BlockStatistics({ hardness: 73491, solid: true, diggablerock: true, damage_fall: 90 });//"говноскал";

const Block = Object.freeze({
    empty: 0,
    gate: 30,// "ворота";//
    vulk: 31,//"вулкан";//
    ground0: 32,//"земля";//GROUND0
    ground1: 33,//"земля1";//GROUND1
    ground2: 34,//"земля2";//GROUND2
    road: 35,//"дорога";
    g_road: 36,//"з-покрытие";
    build_entry: 37,//"вход в здание";
    build_corner: 38,//"угол здания";
    poly: 39,//"полимер";
    bolder_black0: 40,//"черный валун (о)";
    bolder_black1: 41,//"черный валун (к1)";
    bolder_black2: 42,//"черный валун (к2)";
    bolder_metal0: 43,//"мет. валун (к)";
    bolder_metal1: 44,//"мет. валун (с)";
    bolder_metal2: 45,//"мет. валун (ф)";
    block_quadro: 48,//"квадро";
    block_support: 49,//"опора";
    alive_cyan: 50,//"голубая жива";
    alive_red: 51,//"красная жива";
    alive_vio: 52,//"фиол жива";
    alive_black: 53,//"черная жива";
    alive_white: 54,//"белая жива";
    alive_reinbow: 55,//"радужная жива";
    sand_white0: 60,//"белый песок (с)";
    sand_white1: 61,//"белый песок (т)";
    sand_red0: 62,//"красный песок (с)";
    sand_red1: 63,//"красный песок (т)";
    sand_grey0: 64,//"серый песок (с)";
    sand_grey1: 65,//"серый песок (т)";
    slime_white: 66,//"бел слизь";
    slime_vio: 67,//"фиол слизь";
    slime_perl: 68,//"перл";
    cry_x_green: 71,//"зел х4";
    cry_x_blue: 72,//"син х3";
    cry_x_red: 73,//"крась х2";
    cry_x_cyan: 74,//"голь х2";
    cry_x_: 75,//"фиол х2";
    block_war_constr: 80,//"ВБ констр";//
    block_war: 81,//"ВБ";//ВБ
    slime_war: 82,//"ВБ слизь";//
    slime_yellow: 86,//"желтая слизь";
    box: 90,//"бокс";
    magma: 91,//"магма";
    bolder_grey0: 92,//"серый валун (ср)";
    bolder_grey1: 93,//"серый валун (темн)";
    bolder_grey2: 94,//"серый валун (св)";
    sand_blue0: 97,//"синий песок (т)";
    sand_blue1: 98,//"синий песок (с)";
    sand_yellow0: 99,//"желтый песок (т)";
    sand_yellow: 100,//"желтый песок (с)";
    block_green: 101,//"зел. блок";
    block_yellow: 102,//"желт. блок";
    rock_vio: 103,//"фиол скала";
    block_fed: 104,//"фед-блок";
    block_red: 105,//"красн. блок";
    build_frame: 106,//"фрейм здания";//здания
    cry_green: 107,//"зел кри";
    cry_red: 108,//"крас кри";
    cry_blue: 109,//"син кри";
    cry_vio: 110,//"фиол кри";
    cry_white: 111,//"бель кри";
    cry_cyan: 112,//"голь кри";
    rock_blue: 113,//"синий златоскал";
    rock_black: 114,//"черноскал";
    rock_black_green: 115,//"черноскал зеленый";
    alive_blue: 116,//"синяя жива";
    rock_red: 117,//"красноскал";
    rock_acid: 118,//"кислотка";
    rock_hypno: 119,//"гипноскал";
    rock_golden: 120,//"златоскал";
    rock_green: 121,//"зеленая скала";
    rock_shit: 122,//"говноскал";
})

class FallableBlocks {
    static get sand_yellow() { return RandInt(0, 1) ? Block.sand_yellow : Block.sand_yellow0 };
    static get sand_blue() { return RandInt(0, 1) ? Block.sand_blue0 : Block.sand_blue1 };
    static get sand_grey() { return RandInt(0, 1) ? Block.sand_grey0 : Block.sand_grey1 };
    static get sand_red() { return RandInt(0, 1) ? Block.sand_red0 : Block.red1 };
    static get sand_white() { return RandInt(0, 1) ? Block.sand_white0 : Block.sand_white1 };
    static get bolder_black() { return Block.bolder_black0 + RandInt(0, 2) }
    static get bolder_grey() { return Block.bolder_grey0 + RandInt(0, 2) }
    static get bolder_metal() { return Block.bolder_metal0 + RandInt(0, 2) }
}

const SkillType = OF({
    Unknown: -1,
    /**a | aacd | Защита от слизи*/
    AntiSlime: 0,
    /**k | ablk | Анти-блок*/
    AntiBlock: 1,
    /**j | adja | Смежное извлечение*/
    AdjacentExtraction: 2,
    /**U | geol | Геология*/
    Geology: 3,
    /**B | minb | Добыча синих*/
    MineBlue: 4,
    /**G | ming | Добыча зеленых*/
    MineGreen: 5,
    /**D | dest | Разрушение*/
    Destruction: 6,
    /**x | anig | Аннигиляция*/
    Annihilation: 7,
    /**y | crys | Кристаллография*/
    Crystallography: 8,
    /**z | decn | Деконструкция*/
    Deconstruction: 9,
    /**u | agun | Защита от пушек*/
    AntiGun: 10,
    /**E | bldr | Стройка красных*/
    BuildRed: 11,
    /**d | digg | Копание*/
    Digging: 12,
    /**l | live | Защита*/
    Health: 13,
    /**m | mine | Добыча*/
    MineGeneral: 14,
    /**R | minr | Добыча красных*/
    MineRed: 15,
    /**L | bldg | Стройка*/
    BuildGreen: 16,
    /**Q | bldq | Стройка квадроблоков*/
    BuildQuadro: 17,
    /**q | dete | Обнаружение*/
    Detection: 18,
    /**M | moto | Передвижение*/
    Movement: 19,
    /**Y | bldy | Стройка желтых*/
    BuildYellow: 20,
    /**P | comp | Компрессия*/
    Compression: 21,
    /**F | frig | Охлаждение*/
    Fridge: 22,
    /**C | minc | Добыча голубых*/
    MineCyan: 23,
    /**t | moro | Передвижение по дорогам*/
    RoadMovement: 24,
    /**U | upgr | Экспертное обучение*/
    Upgrade: 25,
    /**Z | deac | Деактивация*/
    Deactivation: 26,
    /**h | hcmp | Гиперкомпрессия*/
    HyperPacking: 27,
    /**V | minv | Добыча фиолетовых*/
    MineViolet: 28,
    /**p | pack | Вместимость*/
    Packing: 29,
    /**b | pakb | Упаковка синих*/
    PackingBlue: 30,
    /**c | pakc | Упаковка голубых*/
    PackingCyan: 31,
    /**v | pakv | Упаковка фиолетовых*/
    PackingViolet: 32,
    /**M | mony | Оптимизация*/
    Discount: 33,
    /**J | sort | Сортировка*/
    Sort: 34,
    /**S | subl | Турбо-охлаждение*/
    Turbo: 35,
    /**X | magn | Размагничивание*/
    DeMagnetizing: 36,
    /**W | minw | Добыча белых*/
    MineWhite: 37,
    /**r | pakr | Упаковка красных*/
    PackingRed: 38,
    /**w | pakw | Упаковка белых*/
    PackingWhite: 39,
    /**g | pakg | Упаковка зеленых*/
    PackingGreen: 40,
    /**o | reco | Извлечение*/
    Extraction: 41,
    /**e | repa | Ремонт*/
    Repair: 42,
    /**D | emin | Экспертная добыча*/
    ExpertMining: 43,
    /**i | wash | Промывание*/
    Washing: 44,
    /**f | frac | Дробление*/
    Fracturing: 45,
    /**H | nano | Наноупаковка*/
    NanoPacking: 46,
    /**O | opor | Стройка опор*/
    BuildStructure: 47,
    /**A | road | Стройка дорог*/
    BuildRoad: 48,
    /** *B | bldu | Универсальная стройка*/
    BuildUniversal: 49,
    /** *L | warb | Военный блок*/
    BuildWar: 50,
    /** *A | arch | Архитектура*/
    Architecture: 51,
    /** *T | tods | Тотальное разрушение*/
    TotalDestruction: 52,
    /** *u | ultr | Ультра-добыча белых*/
    UltraWhite: 53,
    /** *J | jewl | Ювелирная добыча фиолетовых*/
    Jewlery: 54,
    /** *I | indu | Индукция*/
    Induction: 55,
    /** *a | acid | Слизевая добыча*/
    MineSlime: 56,
    /** *d | deep | Глубинная добыча*/
    MineDeep: 57,
    /** *g | gluo | Глюонная упаковка*/
    GluonPacking: 58
});

/**@type {Map<Number,String>} */
const SkillName = new Map();
for (const key in SkillType) {
    if (Object.prototype.hasOwnProperty.call(SkillType, key)) {
        let element = SkillType[key];
        SkillName.set(SkillType[key], key);
    }
}
OF(SkillName);

class SkillParams {
    effect;
    cost;
    exp;
    oppCoef = 1000000;
    opp(lvl = 0) { return ~~(lvl * this.cost(lvl) / 2 / this.oppCoef) }
    /**
     * @param {((lvl:Number)=>Number)} effect 
     * @param {((lvl:Number)=>Number)} cost 
     */
    constructor(effect, cost, exp = ()=>{return 100}) {
        this.effect = effect;
        this.cost = cost;
        this.cost = exp;
    }
}

class ProgressFunctions {
    /**
     * 
     * @param {Number} addition (0+) Константа что прибавляется к каждому уровню
     * @param {Number} factor (1+) регулировка скорости роста графика
     * @param {Number} LNCoef  Кривизна функции (1-оригинальная, меньше 1-повышение крутизны, больше 1.4-разворот в обратную сторону)
     * @param {Number} maxResult 
     * @returns {Number}
     */
    static NaturalLog2(addition = 0, factor = 0,LNCoef = 1, maxResult = Number.MAX_SAFE_INTEGER) {
        let func = function (lvl = 0) {
            let result = addition + factor * ~~(lvl ** (Math.LN2 * LNCoef));
            return result <= maxResult ? result : maxResult
        }
        return func;
    }
    static Linear() { }
}

const DEFAULT_SKILL_CALC = new SkillParams((lvl) => { return 100 + lvl * 10 }, (lvl) => { return 1000 * ~~(lvl ** Math.LN2) });
/**@type {SkillParams[]} */
const SkillCalculator = new Array(59);
for (let i = 0; i < SkillCalculator.length; i++) {
    SkillCalculator[i] = DEFAULT_SKILL_CALC;
}

SkillCalculator[SkillType.Digging] = new SkillParams((lvl) => { return 100 + lvl * 10 }, (lvl) => { return 1000 * ~~(lvl ** Math.LN2) });
SkillCalculator[SkillType.Packing] = new SkillParams((lvl) => { return lvl * 100 }, (lvl) => { return ~~(100 * lvl ** Math.LN2) });
SkillCalculator[SkillType.HyperPacking] = new SkillParams((lvl) => { return lvl * 200 }, (lvl) => { return ~~(250 * lvl ** Math.LN2) });
SkillCalculator[SkillType.NanoPacking] = new SkillParams((lvl) => { return lvl * 500 }, (lvl) => { return ~~(1500 * lvl ** Math.LN2) });
SkillCalculator[SkillType.GluonPacking] = new SkillParams((lvl) => { return lvl * 5000 }, (lvl) => { return ~~(1000000 * lvl ** Math.LN2) });

SkillCalculator[SkillType.AntiGun] = new SkillParams((lvl) => { return lvl < 522 ? ~~(lvl ** Math.LN2 * 1.205) : ~~(521 ** Math.LN2 * 1.205) }, (lvl) => { return 2000 + lvl * 1000 })//521lvl 92%
SkillCalculator[SkillType.Deconstruction] = new SkillParams((lvl) => { return lvl < 47 ? ~~(lvl ** Math.LN2 * 6.9) : ~~(47 ** Math.LN2 * 6.9) }, (lvl) => { return 1000 + lvl * 500 })         //47lvl 99%
SkillCalculator[SkillType.Movement] = new SkillParams((lvl) => { return lvl < 566 ? ~~(lvl ** Math.LN2 * 67.37) : ~~(566 ** Math.LN2 * 67.37) }, (lvl) => { return 500 + lvl * 500 })          //566 lvl 54.52km
SkillCalculator[SkillType.RoadMovement] = new SkillParams((lvl) => { return lvl < 143 ? ~~(lvl ** Math.LN2 * 11.3) : ~~(143 ** Math.LN2 * 11.3) }, (lvl) => { return 1000 + lvl * 500 })         //143 lvl 352%
SkillCalculator[SkillType.AntiSlime] = new SkillParams((lvl) => { return lvl < 165 ? ~~(lvl ** Math.LN2 * 2.88) : ~~(165 ** Math.LN2 * 2.88) }, (lvl) => { return 400 + lvl * 500 })           //165 lvl 99%
SkillCalculator[SkillType.AntiBlock] = new SkillParams((lvl) => { return lvl < 162 ? ~~(lvl ** Math.LN2 * 2.68) : ~~(162 ** Math.LN2 * 2.68) }, (lvl) => { return 400 + lvl * 500 })          //162 lvl 91%
SkillCalculator[SkillType.Annihilation] = new SkillParams((lvl) => { return lvl < 61 ? ~~(lvl ** Math.LN2 * 5.73) : ~~(61 ** Math.LN2 * 5.73) }, (lvl) => { return 50 + lvl * 500 })         //61 lvl 99%
SkillCalculator[SkillType.Crystallography] = new SkillParams((lvl) => { return lvl < 47 ? ~~(lvl ** Math.LN2 * 6.9) : ~~(47 ** Math.LN2 * 6.9) }, (lvl) => { return 300 + lvl * 500 })        //47 lvl 99%
SkillCalculator[SkillType.Deactivation] = new SkillParams((lvl) => { return lvl < 47 ? ~~(lvl ** Math.LN2 * 6.9) : ~~(47 ** Math.LN2 * 6.9) }, (lvl) => { return lvl * 500 })           //47 lvl 99%
SkillCalculator[SkillType.Destruction] = new SkillParams((lvl) => { return lvl < 53 ? ~~(lvl ** Math.LN2 * 5.73) : ~~(53 ** Math.LN2 * 5.73) }, (lvl) => { return lvl * 500 })          //53 lvl 89%
SkillCalculator[SkillType.Fracturing] = new SkillParams((lvl) => { return lvl < 65 ? ~~(lvl ** Math.LN2 * 4.93) : ~~(65 ** Math.LN2 * 4.93) }, (lvl) => { return lvl * 500 })           //65 lvl 89%
SkillCalculator[SkillType.DeMagnetizing] = new SkillParams((lvl) => { return lvl < 61 ? ~~(lvl ** Math.LN2 * 5.73) : ~~(61 ** Math.LN2 * 5.73) }, (lvl) => { return lvl * 500 })          //61 lvl 99%
SkillCalculator[SkillType.Discount] = new SkillParams((lvl) => { return lvl < 174 ? ~~(lvl ** Math.LN2 * 1.4) : ~~(174 ** Math.LN2 * 1.4) }, (lvl) => { return 200000 + lvl * 500000 })               //174 lvl 50%
SkillCalculator[SkillType.TotalDestruction] = new SkillParams((lvl) => { return lvl < 170 ? ~~(lvl ** Math.LN2 * 2.82) : ~~(170 ** Math.LN2 * 2.82) }, (lvl) => { return lvl * 500 })   //170 lvl 99%
