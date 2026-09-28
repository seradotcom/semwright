"""Reproduce the original synthesized mono WAVs; no sampled third-party audio."""
import math,random,struct,wave
from pathlib import Path
out=Path(__file__).resolve().parents[1]/'project/assets'
rate=22050
random.seed(3)
for name,duration in [('eat',.13),('dash',.25),('grow',.9),('caught',.45),('ambience_1',8),('ambience_2',8),('ambience_3',8)]:
    samples=[]
    for i in range(int(rate*duration)):
        t=i/rate;u=t/duration
        if name.startswith('ambience'):
            stage=int(name[-1]);frequencies=[110,165,220,277.25,330][:stage+2]
            v=sum(math.sin(math.tau*f*t)*.055 for f in frequencies)*(1+.2*math.sin(math.tau*t/8))
        elif name=='dash':v=(random.random()*2-1)*.18*math.sin(math.pi*u)**2
        else:
            base=520 if name=='eat' else 220 if name=='grow' else 140
            v=.35*math.sin(math.tau*(base*t+(600 if name=='eat' else 150 if name=='grow' else -80)*t*t))*math.sin(math.pi*u)**2
        samples.append(struct.pack('<h',int(max(-1,min(1,v))*32767)))
    with wave.open(str(out/(name+'.wav')),'wb') as w:
        w.setnchannels(1);w.setsampwidth(2);w.setframerate(rate);w.writeframes(b''.join(samples))
