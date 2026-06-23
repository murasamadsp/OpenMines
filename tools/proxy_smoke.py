#!/usr/bin/env python3
"""E2E smoke для openmines-proxy: бесшовный рестарт бэкенда.

Проверяет без игрового auth, что прокси держит клиентское TCP-соединение при
рестарте бэкенда: dummy-бэкенд шлёт ST/AU/PI/cf и логирует приём; raw-клиент
коннектится через прокси, шлёт AU, затем бэкенд убивается и поднимается заново.
Утверждаем: (1) клиентский сокет НЕ закрыт, (2) новый бэкенд получил реплей AU,
(3) клиент получил cf от нового бэкенда (swallow ST/AU/PI отработал).

Запуск: python3 tools/proxy_smoke.py /path/to/target/debug/openmines-proxy
"""
import socket
import struct
import subprocess
import sys
import threading
import time

PROXY_BIND = "127.0.0.1:8090"
BACKEND = "127.0.0.1:8095"
BACKEND_PORT = 8095


def frame(event: bytes, payload: bytes) -> bytes:
    body = b"U" + event + payload
    return struct.pack("<I", 4 + len(body)) + body


def read_frames(sock, want, timeout=5.0):
    """Прочитать >= want байт, вернуть список событий (2B) из кадров."""
    sock.settimeout(timeout)
    buf = b""
    events = []
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            chunk = sock.recv(4096)
        except socket.timeout:
            break
        if not chunk:
            break
        buf += chunk
        while len(buf) >= 4:
            ln = struct.unpack("<I", buf[:4])[0]
            if len(buf) < ln:
                break
            events.append(buf[5:7])
            buf = buf[ln:]
        if len(events) >= want:
            break
    return events


class DummyBackend:
    """Один-коннект бэкенд: при подключении шлёт ST/AU/PI/cf, логирует приём."""

    def __init__(self, port):
        self.port = port
        self.received_events = []
        self.srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.srv.bind(("127.0.0.1", port))
        self.srv.listen(1)
        self.alive = True
        self.t = threading.Thread(target=self._serve, daemon=True)
        self.t.start()

    def _serve(self):
        self.srv.settimeout(1.0)
        while self.alive:
            try:
                conn, _ = self.srv.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            conn.sendall(frame(b"ST", b"ok"))
            conn.sendall(frame(b"AU", b"abcde"))
            conn.sendall(frame(b"PI", b"0:0:"))
            conn.sendall(frame(b"cf", b'{"w":1}'))
            conn.settimeout(0.5)
            buf = b""
            while self.alive:
                try:
                    chunk = conn.recv(4096)
                except socket.timeout:
                    continue
                except OSError:
                    break
                if not chunk:
                    break
                buf += chunk
                while len(buf) >= 4:
                    ln = struct.unpack("<I", buf[:4])[0]
                    if len(buf) < ln:
                        break
                    self.received_events.append(buf[5:7])
                    buf = buf[ln:]
            conn.close()

    def stop(self):
        self.alive = False
        try:
            self.srv.close()
        except OSError:
            pass


def main():
    proxy_bin = sys.argv[1] if len(sys.argv) > 1 else "target/debug/openmines-proxy"

    backend = DummyBackend(BACKEND_PORT)
    time.sleep(0.3)
    proxy = subprocess.Popen(
        [proxy_bin, "--bind", PROXY_BIND, "--backend", BACKEND, "--reconnect-timeout", "10"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    time.sleep(0.6)

    ok = True
    try:
        cli = socket.create_connection(("127.0.0.1", 8090), timeout=5)
        evs = read_frames(cli, want=4)
        print(f"1) client got initial frames: {[e.decode() for e in evs]}")
        assert b"cf" in evs, "клиент не получил cf от первого бэкенда"

        cli.sendall(frame(b"AU", b"1_42_token"))
        time.sleep(0.4)
        assert b"AU" in backend.received_events, "бэкенд не получил клиентский AU"
        print(f"2) backend#1 received: {[e.decode() for e in backend.received_events]}")

        # Рестарт бэкенда — имитация деплоя.
        backend.stop()
        time.sleep(0.4)
        backend2 = DummyBackend(BACKEND_PORT)
        print("3) backend restarted")

        # Клиент НЕ должен быть закрыт; должен получить cf от нового бэкенда.
        evs2 = read_frames(cli, want=1, timeout=6.0)
        print(f"4) client after restart got: {[e.decode() for e in evs2]}")
        assert b"cf" in evs2, "клиент не получил cf от НОВОГО бэкенда (socket closed?)"
        time.sleep(0.3)
        assert b"AU" in backend2.received_events, "новый бэкенд не получил реплей AU"
        print(f"5) backend#2 received (AU replayed): {[e.decode() for e in backend2.received_events]}")
        backend2.stop()
        print("\nPASS: бесшовный рестарт — клиент жив, AU реплейнут, cf доставлен")
    except (AssertionError, OSError) as e:
        ok = False
        print(f"\nFAIL: {e}")
    finally:
        proxy.terminate()
        backend.stop()

    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
