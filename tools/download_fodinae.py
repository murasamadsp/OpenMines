import os
import urllib.request
import urllib.error
import time

BASE_URL = "https://fodinae.online"
TARGET_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "js_reference"))

HTML_CSS_JS = [
    "", # index.html
    "styles/main.css",
    "styles/map.css",
    "scripts/tables/MapTables.js",
    "scripts/tables/DataTables.js",
    "scripts/tables/minestileset.js",
    "scripts/tables/items.js",
    "scripts/usefuladitions.js",
    "scripts/cargo.js",
    "scripts/entitys/entity.js",
    "scripts/entitys/packs.js",
    "scripts/userGUI/guiControls.js",
    "maps/test.js",
    "scripts/wmap.js",
    "scripts/geophys.js",
    "scripts/programmator.js",
    "scripts/bot.js",
    "scripts/wrenderer.js",
    "scripts/userGUI/Inventory.js",
    "scripts/userGUI/Skills.js",
    "scripts/main.js",
    "scripts/controls.js",
    "scripts/userGUI/Mapdrawer.js",
    "scripts/userGUI/Console.js",
]

GRAPHICS = [
    "graphics/skills/BotSkillsBackground.png",
    "graphics/skills/icons/image_part_0.png",
    "graphics/skills/icons/slot_empty.png",
    "graphics/minesTileset.png",
    "graphics/skins.png",
    "graphics/DiggingSpurk.png",
    "graphics/RoboHeal.png",
    "graphics/GeoEffect.png",
]

# Generate item icons range 0 to 50
for i in range(51):
    GRAPHICS.append(f"graphics/inventory/icons/{i}.png")

# Generate skill icons range 1 to 56
for i in range(1, 57):
    GRAPHICS.append(f"graphics/skills/icons/image_part_{i}.png")

def download_file(rel_path):
    url = f"{BASE_URL}/{rel_path}" if rel_path else BASE_URL
    local_path = os.path.join(TARGET_DIR, rel_path if rel_path else "index.html")
    
    # Ensure dir exists
    os.makedirs(os.path.dirname(local_path), exist_ok=True)
    
    headers = {
        "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
    }
    
    req = urllib.request.Request(url, headers=headers)
    print(f"Downloading {url} -> {local_path}... ", end="", flush=True)
    
    # Retry logic
    retries = 3
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=15) as response:
                content = response.read()
                with open(local_path, "wb") as f:
                    f.write(content)
                print(f"SUCCESS ({len(content)} bytes)")
                return True
        except urllib.error.HTTPError as e:
            if e.code == 404:
                print(f"FAILED (404 Not Found)")
                return False
            print(f"ERROR (HTTP {e.code}) - retrying...")
        except Exception as e:
            print(f"ERROR ({str(e)}) - retrying...")
        time.sleep(1)
    
    print("FAILED after retries")
    return False

def main():
    print("Starting download of Fodinae files...")
    all_files = HTML_CSS_JS + GRAPHICS
    success_count = 0
    fail_count = 0
    
    for file in all_files:
        if download_file(file):
            success_count += 1
        else:
            fail_count += 1
            
    print(f"\nDownload completed: {success_count} succeeded, {fail_count} failed.")

if __name__ == "__main__":
    main()
