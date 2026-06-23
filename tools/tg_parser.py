#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import os
import sys
import json
import asyncio
import random
from datetime import datetime
from telethon import TelegramClient, errors
from telethon.tl.types import (
    MessageService,
    MessageMediaPhoto,
    MessageMediaDocument,
    MessageMediaWebPage,
    MessageMediaPoll,
    MessageMediaDice,
    MessageMediaContact,
    MessageMediaGeo,
    MessageMediaVenue,
    DocumentAttributeFilename,
    DocumentAttributeAudio,
    DocumentAttributeVideo
)

# Имена файлов конфигурации и состояния (вычисляются относительно директории скрипта)
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
CONFIG_FILE = os.path.join(SCRIPT_DIR, 'tg_config.json')
STATE_FILE = os.path.join(SCRIPT_DIR, 'tg_state.json')
SESSION_NAME = os.path.join(SCRIPT_DIR, 'tg_parser_session')

# Внутренние константы для сохранения прогресса
SAVE_INTERVAL = 10  # Как часто сбрасывать сообщения на диск и сохранять прогресс

# Настройки имитации человеческого поведения (для защиты от банов)
HUMAN_PAUSE_EVERY_MIN = 120  # Пауза каждые 120-250 сообщений
HUMAN_PAUSE_EVERY_MAX = 250
HUMAN_PAUSE_DURATION_MIN = 5.0  # Длительность паузы в секундах (от 5 до 12 сек)
HUMAN_PAUSE_DURATION_MAX = 12.0

def load_config():
    """Загружает конфигурацию из JSON-файла, если он существует."""
    if os.path.exists(CONFIG_FILE):
        try:
            with open(CONFIG_FILE, 'r', encoding='utf-8') as f:
                return json.load(f)
        except Exception as e:
            print(f"[⚠️ Предупреждение] Не удалось прочитать конфигурацию: {e}")
    return {}

def save_config(config):
    """Сохраняет конфигурацию в JSON-файл."""
    try:
        clean_config = {
            "api_id": config.get("api_id", ""),
            "api_hash": config.get("api_hash", ""),
            "phone": config.get("phone", ""),
            "password": config.get("password", ""),
            "target_group": config.get("target_group", ""),
            "output_file": config.get("output_file", "telegram_history.md"),
            "delay_between_messages": config.get("delay_between_messages", 0.1)
        }
        with open(CONFIG_FILE, 'w', encoding='utf-8') as f:
            json.dump(clean_config, f, indent=4, ensure_ascii=False)
        print(f"[⚙️] Настройки сохранены в {CONFIG_FILE}")
    except Exception as e:
        print(f"[❌ Ошибка] Не удалось сохранить конфигурацию: {e}")

def get_config_from_user():
    """Запрашивает настройки у пользователя в интерактивном режиме."""
    config = load_config()
    
    print("\n=== НАСТРОЙКА TELEGRAM ПАРСЕРА ===")
    print("Для работы скрипта требуются API ID и API Hash.")
    print("Получить их можно на сайте https://my.telegram.org в разделе 'API development tools'.\n")
    
    # API ID
    default_api_id = config.get('api_id', '')
    prompt = f"Введите API ID [{default_api_id}]: " if default_api_id else "Введите API ID: "
    val = input(prompt).strip()
    if val:
        config['api_id'] = int(val)
    elif not default_api_id:
        print("[❌ Ошибка] API ID обязателен для заполнения.")
        sys.exit(1)
        
    # API Hash
    default_api_hash = config.get('api_hash', '')
    prompt = f"Введите API Hash [{default_api_hash}]: " if default_api_hash else "Введите API Hash: "
    val = input(prompt).strip()
    if val:
        config['api_hash'] = val
    elif not default_api_hash:
        print("[❌ Ошибка] API Hash обязателен для заполнения.")
        sys.exit(1)
        
    # Номер телефона
    default_phone = config.get('phone', '')
    prompt = f"Введите ваш номер телефона (например, +79991234567) [{default_phone}]: " if default_phone else "Введите ваш номер телефона (например, +79991234567): "
    val = input(prompt).strip()
    if val:
        config['phone'] = val
    elif not default_phone:
        print("[❌ Ошибка] Номер телефона обязателен для заполнения.")
        sys.exit(1)
        
    # Облачный пароль (2FA)
    default_pwd = config.get('password', '')
    prompt = f"Введите облачный пароль (2FA), если включен [{default_pwd}]: " if default_pwd else "Введите облачный пароль (2FA), если включен (оставьте пустым, если нет): "
    val = input(prompt).strip()
    if val:
        config['password'] = val
        
    # Целевая группа
    default_target = config.get('target_group', '')
    prompt = f"Введите ссылку или юзернейм группы/канала (например, @durov или https://t.me/...) [{default_target}]: " if default_target else "Введите ссылку или юзернейм группы/канала: "
    val = input(prompt).strip()
    if val:
        config['target_group'] = val
    elif not default_target:
        print("[❌ Ошибка] Целевая группа обязательна для заполнения.")
        sys.exit(1)
        
    # Имя выходного файла
    default_output = config.get('output_file', 'telegram_history.md')
    val = input(f"Имя выходного Markdown-файла [{default_output}]: ").strip()
    config['output_file'] = val if val else default_output
    
    # Задержка между сообщениями по умолчанию
    config['delay_between_messages'] = config.get('delay_between_messages', 0.1)
    
    save_config(config)
    return config

def load_state(target_group):
    """Загружает сохраненное состояние парсинга."""
    if os.path.exists(STATE_FILE):
        try:
            with open(STATE_FILE, 'r', encoding='utf-8') as f:
                state = json.load(f)
                if state.get('target_group') == target_group:
                    return state
        except Exception as e:
            print(f"[⚠️ Предупреждение] Не удалось загрузить файл состояния: {e}")
    return {"target_group": target_group, "last_message_id": 0, "total_parsed": 0}

def save_state(state):
    """Сохраняет текущее состояние парсинга."""
    try:
        with open(STATE_FILE, 'w', encoding='utf-8') as f:
            json.dump(state, f, indent=4, ensure_ascii=False)
    except Exception as e:
        print(f"[❌ Ошибка] Не удалось сохранить файл состояния: {e}")

async def get_sender_name(client, message):
    """Определяет имя отправителя сообщения без лишних запросов к API."""
    try:
        sender = await message.get_sender()
        if not sender:
            return "Анонимный отправитель"
        
        if hasattr(sender, 'first_name'):
            first_name = sender.first_name or ""
            last_name = sender.last_name or ""
            username = f" (@{sender.username})" if getattr(sender, 'username', None) else ""
            name = f"{first_name} {last_name}".strip()
            return f"{name}{username}" if name else f"User ID: {sender.id}{username}"
        
        elif hasattr(sender, 'title'):
            return f"{sender.title}"
            
        return "Неизвестный отправитель"
    except Exception:
        if message.sender_id:
            return f"ID отправителя: {message.sender_id}"
        return "Неизвестный отправитель"

def format_document_info(media):
    """Красиво форматирует информацию о документе (файлы, музыка, кругляшки, голосовые)."""
    doc = media.document
    if not doc:
        return "*[📎 Вложение: Документ]*"
    
    filename = "без_имени"
    is_voice = False
    is_round_video = False
    
    for attr in doc.attributes:
        if isinstance(attr, DocumentAttributeFilename):
            filename = attr.file_name
        elif isinstance(attr, DocumentAttributeAudio) and attr.voice:
            is_voice = True
        elif isinstance(attr, DocumentAttributeVideo) and getattr(attr, 'round_message', False):
            is_round_video = True
            
    if is_voice:
        return "*🎤 [Голосовое сообщение]*"
    if is_round_video:
        return "*🎥 [Видеосообщение (кругляшок)]*"
        
    mime = doc.mime_type or ""
    if mime.startswith('image/'):
        return f"*🖼️ [Изображение: {filename}]*"
    elif mime.startswith('video/'):
        return f"*🎬 [Видео: {filename}]*"
    elif mime.startswith('audio/'):
        return f"*🎵 [Аудиозапись: {filename}]*"
        
    return f"*📎 [Файл: {filename}]*"

def format_webpage_info(media):
    """Форматирует превью ссылки (WebPage)."""
    wp = media.webpage
    if not wp or isinstance(wp, type(None)):
        return ""
    
    title = getattr(wp, 'title', '') or ''
    description = getattr(wp, 'description', '') or ''
    url = getattr(wp, 'url', '') or ''
    
    info = []
    if title:
        info.append(f"**{title}**")
    if description:
        desc = description[:250] + "..." if len(description) > 250 else description
        info.append(desc)
    if url:
        info.append(f"[Открыть ссылку]({url})")
        
    if info:
        return "🔗 **Превью ссылки:**\n> " + "\n> ".join(info)
    return ""

def format_poll_info(media):
    """Форматирует опрос со всеми вариантами ответов."""
    poll = media.poll
    question = poll.question
    answers = []
    
    for answer in poll.answers:
        answers.append(f"- {answer.text}")
        
    answers_str = "\n".join(answers)
    poll_type = "Публичный опрос" if not poll.quiz else "Викторина"
    return f"📊 **[{poll_type}] {question}**\nВарианты ответов:\n{answers_str}"

def format_contact_info(media):
    """Форматирует карточку контакта."""
    first_name = getattr(media, 'first_name', '') or ''
    last_name = getattr(media, 'last_name', '') or ''
    name = f"{first_name} {last_name}".strip()
    phone = getattr(media, 'phone_number', '') or ''
    return f"👤 **[Поделился контактом]** {name} ({phone})"

def format_geo_info(media):
    """Форматирует геопозицию в виде ссылки на Google Карты."""
    geo = media.geo
    if not geo:
        return "📍 **[Геопозиция]**"
    lat = getattr(geo, 'lat', 0.0)
    long = getattr(geo, 'long', 0.0)
    return f"📍 **[Геопозиция]** Широта: {lat}, Долгота: {long} | [Посмотреть на Google Картах](https://www.google.com/maps?q={lat},{long})"

def format_service_action(action):
    """Переводит основные системные действия в группах на русский язык."""
    action_type = type(action).__name__
    
    actions_map = {
        'MessageActionChatCreate': "создал(а) группу",
        'MessageActionChatEditTitle': f"изменил(а) название группы на '{getattr(action, 'title', '')}'",
        'MessageActionChatEditPhoto': "изменил(а) аватарку группы",
        'MessageActionChatDeletePhoto': "удалил(а) аватарку группы",
        'MessageActionChatAddUser': "добавил(а) новых участников",
        'MessageActionChatDeleteUser': "вышел(а) из группы (или удален)",
        'MessageActionChatJoinedByLink': "присоединился(ась) по ссылке-приглашению",
        'MessageActionPinMessage': "закрепил(а) сообщение",
        'MessageActionChatMigrateTo': "группа перенесена в супергруппу",
        'MessageActionChannelCreate': "создал(а) канал",
    }
    
    return actions_map.get(action_type, f"совершил системное действие: {action_type}")

def format_markdown_message(message, sender_name):
    """Собирает и форматирует сообщение в красивый Markdown-блок."""
    date_str = message.date.strftime('%Y-%m-%d %H:%M:%S')
    header = f"### 📅 {date_str} | 👤 {sender_name}"
    text = message.text or ""
    extra_content = []
    
    if isinstance(message, MessageService):
        text = f"⚙️ *{format_service_action(message.action)}*"
    elif message.media:
        media = message.media
        if isinstance(media, MessageMediaPhoto):
            extra_content.append("*🖼️ [Прикреплено изображение]*")
        elif isinstance(media, MessageMediaDocument):
            extra_content.append(format_document_info(media))
        elif isinstance(media, MessageMediaWebPage):
            wp_info = format_webpage_info(media)
            if wp_info:
                extra_content.append(wp_info)
        elif isinstance(media, MessageMediaPoll):
            extra_content.append(format_poll_info(media))
        elif isinstance(media, MessageMediaDice):
            extra_content.append(f"🎲 *[Выпало значение: {media.value} в эмодзи {media.emoticon}]*")
        elif isinstance(media, MessageMediaContact):
            extra_content.append(format_contact_info(media))
        elif isinstance(media, MessageMediaGeo) or isinstance(media, MessageMediaVenue):
            extra_content.append(format_geo_info(media))
        else:
            media_class = type(media).__name__.replace('MessageMedia', '')
            extra_content.append(f"*[Вложение: {media_class}]*")

    full_body = text.strip()
    if extra_content:
        extra_str = "\n\n".join(extra_content)
        if full_body:
            full_body = f"{full_body}\n\n{extra_str}"
        else:
            full_body = extra_str
            
    if not full_body:
        full_body = "*(пустое сообщение)*"
        
    return f"{header}\n\n{full_body}\n\n---\n\n"

async def main():
    # Пробуем загрузить существующий конфиг
    config = load_config()
    
    # Проверяем наличие всех обязательных полей
    required_fields = ['api_id', 'api_hash', 'phone', 'target_group']
    is_valid = all(config.get(field) for field in required_fields)
    
    # Если конфиг неполный или не валидный, запрашиваем ввод интерактивно
    if not is_valid:
        config = get_config_from_user()
        
    api_id = config['api_id']
    api_hash = config['api_hash']
    phone = config['phone']
    password = config.get('password', '')
    target_group = config['target_group']
    output_file = config['output_file']
    delay_msg = config.get('delay_between_messages', 0.1)
    
    print("\n[🔌] Инициализация подключения к Telegram...")
    
    # Продвинутая эмуляция официального Mac-клиента Telegram для максимальной безопасности аккаунта.
    client = TelegramClient(
        SESSION_NAME, 
        api_id, 
        api_hash,
        device_model="MacBook Pro",
        system_version="macOS 14.5",
        app_version="10.11.1",
        lang_code="ru",
        system_lang_code="ru-RU",
        flood_sleep_threshold=24 * 3600
    )
    
    try:
        # Если пароль указан в конфигурации, передаем его напрямую в start(),
        # чтобы избежать интерактивного getpass(), который сбоит в неинтерактивных терминалах
        if password:
            print("[🔒] Обнаружен сохраненный пароль 2FA, используем его для авторизации...")
            await client.start(phone=phone, password=password)
        else:
            await client.start(phone=phone)
    except Exception as e:
        print(f"[❌ Ошибка авторизации] Не удалось войти в аккаунт: {e}")
        return
        
    if not await client.is_user_authorized():
        print("[❌ Ошибка] Не удалось авторизоваться.")
        return
        
    print("[✅] Успешная авторизация!")
    
    # Затираем сохраненный пароль в файле tg_config.json после успешной авторизации
    if config.get('password'):
        print("[🧹] Безопасность: стираем пароль 2FA из файла tg_config.json...")
        config['password'] = ""
        save_config(config)
    
    print(f"[🔍] Ищем группу/канал '{target_group}'...")
    try:
        entity = await client.get_entity(target_group)
        chat_title = getattr(entity, 'title', 'Канал/Группа')
        print(f"[🎉] Группа найдена: '{chat_title}' (ID: {entity.id})")
    except Exception as e:
        print(f"[❌ Ошибка] Не удалось найти указанный чат. Проверьте ссылку или юзернейм.")
        print(f"Детали ошибки: {e}")
        await client.disconnect()
        return

    # Загружаем сохраненный прогресс
    state = load_state(target_group)
    last_id = state['last_message_id']
    total_parsed = state.get('total_parsed', 0)
    
    file_exists = os.path.exists(output_file)
    mode = 'a' if (last_id > 0 and file_exists) else 'w'
    
    if mode == 'w':
        print(f"[📝] Создаем новый файл экспорта: {output_file}")
        with open(output_file, 'w', encoding='utf-8') as f:
            f.write(f"# История сообщений: {chat_title}\n")
            f.write(f"Экспорт начат: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
            f.write(f"Источник: `{target_group}`\n\n")
            f.write("---\n\n")
    else:
        print(f"[📝] Продолжаем экспорт в {output_file} с сообщения ID {last_id}")

    print("[🚀] Запуск ПОЛНОЙ выгрузки истории. Нажмите Ctrl+C для безопасной паузы.\n")
    
    # Получаем все сообщения (limit=None) от старых к новым (reverse=True)
    messages_iter = client.iter_messages(
        entity,
        reverse=True,
        min_id=last_id,
        limit=None
    )
    
    unsaved_count = 0
    buffer = []
    current_last_id = last_id
    
    messages_until_human_pause = random.randint(HUMAN_PAUSE_EVERY_MIN, HUMAN_PAUSE_EVERY_MAX)
    processed_in_session = 0
    
    try:
        async for message in messages_iter:
            sender_name = await get_sender_name(client, message)
            formatted_msg = format_markdown_message(message, sender_name)
            buffer.append(formatted_msg)
            
            current_last_id = message.id
            unsaved_count += 1
            total_parsed += 1
            processed_in_session += 1
            
            # 1. Рандомизация микрозадержки
            if delay_msg > 0:
                jitter_delay = delay_msg * random.uniform(0.7, 1.4)
                await asyncio.sleep(jitter_delay)
                
            # 2. Сохранение прогресса на диск
            if unsaved_count >= SAVE_INTERVAL:
                with open(output_file, 'a', encoding='utf-8') as f:
                    f.write("".join(buffer))
                
                buffer.clear()
                unsaved_count = 0
                
                state['last_message_id'] = current_last_id
                state['total_parsed'] = total_parsed
                save_state(state)
                
                msg_date = message.date.strftime('%Y-%m-%d %H:%M:%S')
                print(f"[📈] Выгружено сообщений: {total_parsed} (дата: {msg_date})")
                
            # 3. Имитация "человеческого" отдыха
            if processed_in_session >= messages_until_human_pause:
                if buffer:
                    with open(output_file, 'a', encoding='utf-8') as f:
                        f.write("".join(buffer))
                    buffer.clear()
                    unsaved_count = 0
                    state['last_message_id'] = current_last_id
                    state['total_parsed'] = total_parsed
                    save_state(state)
                
                pause_duration = random.uniform(HUMAN_PAUSE_DURATION_MIN, HUMAN_PAUSE_DURATION_MAX)
                print(f"[☕] Имитируем человеческую паузу: отдыхаем {pause_duration:.1f} сек...")
                await asyncio.sleep(pause_duration)
                
                messages_until_human_pause = random.randint(HUMAN_PAUSE_EVERY_MIN, HUMAN_PAUSE_EVERY_MAX)
                processed_in_session = 0
                    
        # Запись остатков при окончании
        if buffer:
            with open(output_file, 'a', encoding='utf-8') as f:
                f.write("".join(buffer))
            state['last_message_id'] = current_last_id
            state['total_parsed'] = total_parsed
            save_state(state)
            buffer.clear()
            
        print(f"\n[🏁] Успех! Вся история группы выгружена полностью.")
        print(f"Всего сохранено сообщений: {total_parsed}")
        print(f"Результат сохранен в: {output_file}")
        
    except asyncio.CancelledError:
        print("\n[⏸️] Выгрузка приостановлена пользователем.")
        if buffer:
            with open(output_file, 'a', encoding='utf-8') as f:
                f.write("".join(buffer))
            state['last_message_id'] = current_last_id
            state['total_parsed'] = total_parsed
            save_state(state)
            print(f"[💾] Успешно сохранено на диске перед паузой: {len(buffer)} сообщений.")
    except Exception as e:
        print(f"\n[❌ Ошибка во время выгрузки]: {e}")
        if buffer:
            try:
                with open(output_file, 'a', encoding='utf-8') as f:
                    f.write("".join(buffer))
                state['last_message_id'] = current_last_id
                state['total_parsed'] = total_parsed
                save_state(state)
            except Exception:
                pass
    finally:
        await client.disconnect()
        print("[🔌] Отключение от Telegram.")

if __name__ == '__main__':
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\n[⏸️] Программа завершена пользователем.")
