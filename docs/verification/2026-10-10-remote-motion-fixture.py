"""Disposable topmost motion and timestamp fixture, restricted to its own window."""
import ctypes, json, os, sys, threading, time
import tkinter as tk

ctypes.windll.shcore.SetProcessDpiAwareness(2)
geometry=json.loads(os.environ['XCHAT_QA_MONITOR'])
width,height=geometry['size']['width'],geometry['size']['height']
left,top=geometry['position']['x'],geometry['position']['y']
root=tk.Tk();root.title('XChat temporary remote video measurement')
root.overrideredirect(True)
root.geometry(f'{width}x{height}{left:+d}{top:+d}')
root.attributes('-topmost',True)
canvas=tk.Canvas(root,width=width,height=height,bg='#18333d',highlightthickness=0)
canvas.pack(fill='both',expand=True)
label=canvas.create_text(48,64,anchor='nw',text='XChat remote video measurement — closes automatically',fill='white',font=('Arial',28))
tile=canvas.create_rectangle(48,560,348,800,fill='#25b399',outline='')
bits=[canvas.create_rectangle(40+index*16,380,56+index*16,420,outline='') for index in range(48)]
began=time.perf_counter();frames=0;stopped=threading.Event();next_frame=began;last_report=began;previous_code=None
fps=int(os.environ.get('XCHAT_QA_FPS','60'))
if fps not in [30,60]:raise ValueError('QA frame rate must be 30 or 60')
def read_stop():
    if sys.stdin.readline().strip()=='stop':stopped.set()
threading.Thread(target=read_stop,daemon=True).start()
def tick():
    global frames,next_frame,last_report,previous_code
    now=time.perf_counter()
    if stopped.is_set() or now-began>120:
        print(json.dumps(dict(frames=frames,seconds=now-began,callback_fps=frames/(now-began))),flush=True)
        root.destroy();return
    # A moving desktop region keeps the source cheap enough for a 60 Hz target;
    # repainting full-height stripes previously limited this Tk fixture to 28 Hz.
    x=48+((now-began)*260)%min(1000,width-348)
    canvas.coords(tile,x,560,x+300,800)
    code=f'{0xd52a:016b}'+f'{int(time.time()*1000)&0xffffffff:032b}'
    for index,(item,bit) in enumerate(zip(bits,code)):
        if previous_code is None or bit!=previous_code[index]:canvas.itemconfigure(item,fill='white' if bit=='1' else 'black')
    previous_code=code
    # Canvas redraw is an idle task. Flush it before scheduling the next frame,
    # and skip missed deadlines instead of creating a catch-up timer backlog.
    root.update_idletasks()
    frames+=1
    now=time.perf_counter()
    if frames==1 or now-last_report>=5:
        print(json.dumps(dict(frames=frames,seconds=now-began,callback_fps=frames/max(.001,now-began),mapped=bool(root.winfo_ismapped()),width=root.winfo_width(),height=root.winfo_height())),flush=True)
        last_report=now
    next_frame=max(next_frame+1/fps,now+1/fps if next_frame<now-1/fps else now+.001)
    root.after(max(1,round((next_frame-now)*1000)),tick)
root.after(1,tick)
root.mainloop()
