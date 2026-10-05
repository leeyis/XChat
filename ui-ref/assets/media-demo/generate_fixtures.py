"""Create small original media fixtures for the XChat review prototype.

Requires Pillow and FFmpeg only for fixture generation, not for XChat itself.
"""

from pathlib import Path
import math
import struct
import subprocess
import wave

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parent
FONT_PATH = Path("C:/Windows/Fonts/segoeui.ttf")


def font(size):
    return ImageFont.truetype(str(FONT_PATH), size) if FONT_PATH.exists() else ImageFont.load_default()


def make_audio():
    rate, duration = 22050, 12
    samples = bytearray()
    notes = [261.63, 329.63, 392.00, 523.25, 392.00, 329.63]
    for index in range(rate * duration):
        time = index / rate
        note_time = time % 0.8
        frequency = notes[int(time / 0.8) % len(notes)]
        envelope = min(1.0, note_time / 0.025) * math.exp(-note_time * 5)
        value = envelope * (math.sin(2 * math.pi * frequency * time) + 0.18 * math.sin(4 * math.pi * frequency * time))
        samples.extend(struct.pack("<h", int(5200 * value)))
    with wave.open(str(ROOT / "preview-chime.wav"), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(rate)
        output.writeframes(samples)


def video_frame(time):
    image = Image.new("RGB", (640, 360), "#12392d")
    draw = ImageDraw.Draw(image)
    for x in range(24, 640, 24):
        for y in range(24, 360, 24):
            draw.ellipse((x, y, x + 1, y + 1), fill="#245141")
    draw.text((32, 26), "XChat", font=font(27), fill="#f1fff6")
    draw.text((32, 65), "CONNECTED THROUGH YOUR LOCAL NETWORK", font=font(11), fill="#93b7a7")
    nodes = [(155, 200), (320, 154), (486, 219)]
    for start, end in zip(nodes, nodes[1:]):
        draw.line((start, end), fill="#427a60", width=2)
        phase = (time / 2) % 1
        point = (start[0] + (end[0] - start[0]) * phase, start[1] + (end[1] - start[1]) * phase)
        draw.ellipse((point[0] - 4, point[1] - 4, point[0] + 4, point[1] + 4), fill="#9aecbc")
    for number, (x, y) in enumerate(nodes):
        radius = 38 + 3 * math.sin(time * 2 + number)
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), outline="#508c6c", width=1)
        draw.rounded_rectangle((x - 27, y - 22, x + 27, y + 22), radius=9, fill="#e7fff0")
        draw.line((x - 15, y - 7, x + 14, y - 7), fill="#18ac71", width=3)
        draw.line((x - 15, y + 2, x + 6, y + 2), fill="#18ac71", width=3)
        draw.text((x - 25, y + 49), ["LAPTOP", "DESKTOP", "PHONE"][number], font=font(10), fill="#b6dbc8")
    draw.text((32, 319), "MEDIA PREVIEW", font=font(11), fill="#b6dbc8")
    draw.text((540, 319), f"00:{int(time):02d} / 00:08", font=font(10), fill="#b6dbc8")
    return image


def make_video():
    video_frame(0).save(ROOT / "network-preview-poster.png")
    command = [
        "rtk", "proxy", "ffmpeg", "-y", "-loglevel", "error",
        "-f", "rawvideo", "-pixel_format", "rgb24", "-video_size", "640x360",
        "-framerate", "24", "-i", "pipe:0", "-i", str(ROOT / "preview-chime.wav"),
        "-t", "8", "-c:v", "libx264", "-preset", "fast", "-crf", "26",
        "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "64k",
        "-movflags", "+faststart", str(ROOT / "network-preview.mp4"),
    ]
    process = subprocess.Popen(command, stdin=subprocess.PIPE, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    try:
        for index in range(8 * 24):
            process.stdin.write(video_frame(index / 24).tobytes())
    finally:
        process.stdin.close()
    if process.wait() != 0:
        raise RuntimeError("FFmpeg failed to encode the review video")


def make_animation():
    frames = []
    for index in range(24):
        image = Image.new("RGB", (280, 180), "#eaf8ef")
        draw = ImageDraw.Draw(image)
        offset = int(math.sin(index * math.pi / 12) * 7)
        draw.rounded_rectangle((48, 32 + offset, 168, 98 + offset), radius=17, fill="#18ac71")
        draw.polygon([(62, 91 + offset), (62, 112 + offset), (85, 94 + offset)], fill="#18ac71")
        draw.text((79, 43 + offset), "OK!", font=font(32), fill="white")
        draw.rounded_rectangle((137, 94 - offset, 235, 136 - offset), radius=12, fill="#ffffff")
        for x in (158, 181, 204):
            draw.ellipse((x, 108 - offset, x + 8, 116 - offset), fill="#18ac71")
        star_size = 3 + int(2 * (1 + math.sin(index * math.pi / 12)))
        for x, y in [(219, 44), (33, 123)]:
            draw.line((x - star_size, y, x + star_size, y), fill="#74bc91", width=2)
            draw.line((x, y - star_size, x, y + star_size), fill="#74bc91", width=2)
        frames.append(image)
    frames[0].save(ROOT / "chat-motion-poster.png")
    frames[0].save(ROOT / "chat-motion.gif", save_all=True, append_images=frames[1:], duration=80, loop=0, optimize=True)


if __name__ == "__main__":
    make_audio()
    make_video()
    make_animation()
    for path in sorted(ROOT.iterdir()):
        if path.suffix in (".wav", ".mp4", ".gif", ".png"):
            print(f"{path.name}: {path.stat().st_size:,} bytes")
