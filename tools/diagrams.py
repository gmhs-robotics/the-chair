#!/usr/bin/env python3
"""Generate accessible documentation diagrams; schematic, never to scale."""
from html import escape
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "docs/diagrams"
BLUE, GREEN, RED, AMBER = "#086baf", "#087f72", "#b32d45", "#865900"

class Diagram:
    def __init__(self, number, title, subtitle, height, plain=False):
        self.height = height
        self.plain = plain
        self.parts = [
            f"<svg xmlns='http://www.w3.org/2000/svg' width='1200' height='{height}' viewBox='0 0 1200 {height}' role='img' aria-labelledby='title desc'>",
            f"<title id='title'>{escape(title)}</title><desc id='desc'>{escape(subtitle)}</desc>",
            "<defs><marker id='arrow' viewBox='0 0 10 10' refX='9' refY='5' markerWidth='7' markerHeight='7' orient='auto-start-reverse'><path d='M0 0 L10 5 L0 10z' fill='context-stroke'/></marker></defs>",
            f"<rect width='1200' height='{height}' fill='#f4f7fa'/><rect width='1200' height='8' fill='{BLUE}'/>"]
        if not plain:
            self.text(40,44,f"THE CHAIR   /   FIELD GUIDE   /   {number}",13,BLUE,True)
            self.text(40,88,title,32,bold=True)
            self.text(40,120,subtitle,17,"#536879")
    def text(self,x,y,value,size=17,color="#142b3c",bold=False):
        self.parts.append(f"<text x='{x}' y='{y}' font-family='DejaVu Sans, sans-serif' font-size='{size}' font-weight='{700 if bold else 400}' fill='{color}'>{escape(value)}</text>")
    def card(self,x,y,w,h,label,title,lines,color=BLUE):
        self.parts.append(f"<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='12' fill='white' stroke='#cbd7e0'/><rect x='{x}' y='{y+16}' width='5' height='{h-32}' rx='2' fill='{color}'/>")
        self.text(x+22,y+30,label,13,color,True)
        self.text(x+22,y+60,title,23,bold=True)
        for i,line in enumerate(lines): self.text(x+22,y+91+i*27,line,17,"#536879")
    def line(self,points,color=BLUE,arrow=False):
        marker = "marker-end='url(#arrow)'" if arrow else ""
        self.parts.append(f"<polyline points='{points}' fill='none' stroke='{color}' stroke-width='3' stroke-linejoin='round' {marker}/>")
    def note(self,y,title,lines,color=AMBER):
        self.parts.append(f"<rect x='40' y='{y}' width='1120' height='{60+27*len(lines)}' rx='10' fill='#e9eff4'/>")
        self.text(60,y+29,title,17,color,True)
        for i,line in enumerate(lines): self.text(60,y+57+27*i,line,16,"#536879")
    def save(self,name):
        if not self.plain:
            self.text(40,self.height-22,"SCHEMATIC • NOT TO SCALE",12,"#536879",True)
            self.text(850,self.height-22,"V5 / THREE-BRAIN CONFIGURATION",12,"#536879")
        (OUT/name).write_text("\n".join(self.parts)+"\n</svg>\n")

def topology():
    d=Diagram("01","Three-Brain control layout","The master calculates commands; each drivetrain Brain controls and monitors four motors.",680,plain=True)
    d.card(350,40,500,195,"MASTER / SLOT 1","Reads controls and commands both sides",["P17 throttle • P21 steering • optional radio","Runs HUD, arming, drive mixing and system safety","P19 sends LEFT • P20 sends RIGHT"])
    d.line("475,235 475,300 295,300 295,350",arrow=True)
    d.line("725,235 725,300 905,300 905,350",arrow=True)
    d.text(401,275,"COMMANDS DOWN • HEALTH BACK",17,BLUE,True)
    d.card(40,350,510,230,"LEFT / SLOT 2","Controls four LEFT motors",["P21 receives target and returns telemetry","P4–P7 drive the LEFT side","Checks command lease, motors and battery"],GREEN)
    d.card(650,350,510,230,"RIGHT / SLOT 3","Controls four RIGHT motors",["P21 receives target and returns telemetry","P4–P7 drive the RIGHT side","Checks command lease, motors and battery"],GREEN)
    d.save("system-topology.svg")

def ports():
    d=Diagram("02","Steering-wheel connections","ADI letters belong to the master. Smart Port 21 has a different job on each Brain.",1040)
    d.card(410,300,380,225,"MASTER","V5 Brain",["P19 = LEFT data link","P20 = RIGHT data link","P21 = steering motor","P18 = controller radio"])
    for y,letter,title,lines,color in [(170,"ADI A","Left button",["WHEEL: hold to drive","CONTROLLER: brake"],BLUE),(360,"ADI B","Right button",["Rider E-stop","Active in both modes"],RED),(550,"SMART P17","Rotation throttle",["0–270° forward","ADI C unused"],BLUE)]:
        d.card(40,y,310,160,letter,title,lines,color)
    d.line("350,250 380,250 380,354 410,354")
    d.line("350,440 410,440",RED)
    d.line("350,630 380,630 380,495 410,495")
    d.card(850,220,310,160,"SMART P18","V5 radio",["Optional in WHEEL","Required in CONTROLLER"])
    d.card(850,465,310,160,"SMART P21","Steering motor",["Encoder + feedback","1.2 V / 0.4 A cap"])
    d.line("790,354 820,354 820,300 850,300")
    d.line("790,495 820,495 820,545 850,545")
    d.line("495,525 495,742 295,742 295,775",GREEN)
    d.line("705,525 705,742 905,742 905,775",GREEN)
    d.text(505,718,"P19",17,GREEN,True)
    d.text(715,718,"P20",17,GREEN,True)
    d.card(100,775,390,110,"LEFT CHILD","Connect to P21",[],GREEN)
    d.card(710,775,390,110,"RIGHT CHILD","Connect to P21",[],GREEN)
    d.note(910,"RIDER CONTROLS REMAIN ATTACHED",["Buttons are active LOW. A broken E-stop wire can look released; this is not a monitored safety circuit."],RED)
    d.save("controller-brain-ports.svg")

def operation():
    d=Diagram("03","From parked to driving","Boot defaults to WHEEL. Changing modes never transfers a live throttle request.",1040)
    d.card(40,170,340,180,"01 / START","Master first",["Start children manually.","Wait for healthy LEFT + RIGHT.","Center wheel; tap CENTER."])
    d.card(430,170,340,180,"02 / PREPARE","Neutral for 500 ms",["P17 zero; sticks centered.","Release enable and brake.","Wheel near calibrated center."])
    d.card(820,170,340,180,"03 / ARM","Tap ARM or press A",["Both nodes healthy and cool.","CONTROLLER needs radio.","Then press drive enable."])
    d.line("380,258 430,258",arrow=True)
    d.line("770,258 820,258",arrow=True)
    d.line("990,350 990,400 300,400 300,440",arrow=True)
    d.line("990,400 900,400 900,440",arrow=True)
    d.card(40,440,520,220,"WHEEL","Hold left; rotate P17 throttle",["Positive forward; negative reverse.","Release left button: immediate brake.","ARMED remains; hold again to resume.","Use PARK to disarm."],GREEN)
    d.card(640,440,520,220,"CONTROLLER","Hold A/R1; use both sticks",["A arms + enables; R1 also enables.","Left stick up: forward. Right X: steer.","Release enable: brake; remains armed.","L1 or rider left: brake + disarm."],GREEN)
    d.note(704,"AFTER A NORMAL STOP",["Return controls to neutral, stop, release buttons, then ARM again.","MODE / X while armed parks first. Once stopped and neutral, select the mode again."],BLUE)
    d.note(844,"RIDER RIGHT BUTTON OR CONTROLLER B: LATCHED E-STOP",["Release cannot resume motion. Repair the cause and restart all three programs.","Radio loss in CONTROLLER also latches; there is no automatic WHEEL fallback."],RED)
    d.save("driving-flow.svg")

def node():
    d=Diagram("04","Drive-node command lifecycle","Link first, then recoverable motor discovery. First zero starts the 150 ms command lease.",1080)
    d.card(40,170,510,155,"BOOT","Open P21; block motor output",["Do no Smart Motor polling before handshake.","Startup motor WAIT never latches a fault."],GREEN)
    d.card(650,170,510,155,"ASSIGN","Wait for master on P21",["Accept protocol v3, side and session.","Initialization health grants no motion authority."])
    d.line("550,246 650,246",arrow=True)
    d.card(650,410,510,180,"FIRST COMMAND","Accept zero; start lease",["Lost ACK may be retried before first zero.","Every later sequence must advance.","Duplicate SYN cannot renew an active lease."])
    d.line("905,325 905,410",arrow=True)
    d.card(40,410,510,180,"DISCOVER / RUN","Discover, then supervise",["Probe one motor per iteration; wait for 4 stable.","After ready, check motor, battery, loop and duty.","Apply bounded ramp or immediate Brake."],GREEN)
    d.line("650,493 550,493",arrow=True)
    d.line("40,493 20,493 20,375 295,375 295,410",GREEN,True)
    d.text(70,366,"REPEAT WHILE HEALTHY",14,GREEN,True)
    d.line("295,590 295,660 600,660 600,710",RED,True)
    d.line("905,590 905,660 600,660",RED)
    d.text(478,643,"ANY DETECTED FAULT",16,RED,True)
    d.card(230,710,740,175,"LATCHED STOP","Brake now; keep reporting health",["Retry local Brake every nominal 10 ms.","No command, reconnect or cooling clears the latch.","Repair, then restart all three programs."],RED)
    d.note(925,"LOCAL PROTECTION HAS A HARDWARE LIMIT",["A frozen CPU, failed SDK or lost supply may prevent braking. Software is not an independent power cut."])
    d.save("drive-nodes.svg")

def signals():
    d=Diagram("06","From rider input to motor voltage","Control flow, not electrical wiring. Positive steering requests a right turn.",1080)
    d.card(40,165,520,170,"WHEEL INPUT","P17 throttle + physical wheel",["Rotation Sensor: signed 10% steps; ADI A: enable.","P21 encoder: 10° deadzone; full at ±80°.","ADI B: latched E-stop in either mode."])
    d.card(640,165,520,170,"CONTROLLER INPUT","Radio on master P18",["Left Y up: throttle; right X: steering.","A: arm once; sticks drive; L1: coast.","B: E-stop; X: mode; no automatic fallback."])
    d.line("300,335 300,375 600,375 600,415",arrow=True)
    d.line("900,335 900,375 600,375")
    d.card(230,415,740,170,"MASTER / control.rs","Select mode and enforce arming",["Healthy + stopped + cool + centered; neutral for 500 ms.","Only the selected mode supplies throttle and steering.","Rider stops and connected-controller B / L1 remain active."])
    d.line("600,585 600,605 307,605 307,625",arrow=True)
    d.card(40,625,535,180,"MASTER / MIX + SPEED CONTROL","Signed LEFT / RIGHT targets",["WHEEL blends straight drive into a pivot.","Full wheel: LEFT +1 / RIGHT −1 (or reverse).","Feed-forward + RPM error; ramp and cap at ±12 V."],GREEN)
    d.card(625,625,535,180,"CHILD / LOCAL SUPERVISION","Validate, ramp, drive P4–P7",["Receive voltage, not RPM, over the Smart Cable.","Check lease, health, duty; own ±12 V ramp.","Zero commands request immediate Coast."],GREEN)
    d.line("575,715 625,715",GREEN,True)
    d.note(850,"STEERING FEEDBACK AND DASHBOARD",["Master P21 provides a limited spring/damper in WHEEL; follows steering target in CONTROLLER.","Disarming removes powered steering feedback. Zero throttle while armed retains it.","HUD shows measured wheel position and P17 throttle input, even when CONTROLLER drives."],BLUE)
    d.text(40,1018,"Sources: controller/{main,control,steering,hud}.rs • shared/link/drivetrain.rs",14,"#536879")
    d.save("control-signals.svg")

def timing():
    d=Diagram("07","Two independent communication deadlines","Example exchange for one child. Command and health traffic interleave; spacing is not elapsed time.",1130)
    d.text(120,180,"MASTER P19 / P20",20,BLUE,True)
    d.text(790,180,"CHILD P21",20,GREEN,True)
    d.line("260,205 260,735")
    d.line("930,205 930,735",GREEN)
    rows=[
        (245,"SYN: protocol v3 + side + session",True,BLUE),
        (305,"ACK: drivetrain identity + same assignment",False,GREEN),
        (385,"Health request: session + sequence; starts reply deadline",True,BLUE),
        (455,"First accepted SetVoltage(0): starts command lease",True,BLUE),
        (555,"Health reply: matching session + pending sequence",False,GREEN),
        (625,"SetVoltage: session + advancing sequence; renews lease",True,BLUE),
    ]
    for y,label,outbound,color in rows:
        d.text(290,y-12,label,16,color)
        d.line(f"{260 if outbound else 930},{y} {930 if outbound else 260},{y}",color,True)
    d.text(290,705,"Health traffic does not renew the child's command lease.",16,RED,True)
    d.text(290,733,"Readiness waits for health reporting an accepted command.",14,"#536879")
    d.card(40,770,535,180,"CHILD COMMAND LEASE","150 ms since accepted command",["First command must be zero; then keep sending.","Check expiry before reading buffered packets.","Expiry latches; local Brake is retried."],RED)
    d.card(625,770,535,180,"MASTER HEALTH DEADLINE","500 ms from pending request",["Request/retry interval: 100 ms; one sequence per link.","Check deadline before reading queued replies.","Timeout latches; retry stops to both children."],RED)
    d.note(975,"DEADLINES ARE NOT STOPPING-DISTANCE GUARANTEES",["Nominal loop: 10 ms; a >100 ms loop gap trips when execution resumes.","Sources: shared/link/{master,child}.rs, shared/link.rs, shared/safety.rs."])
    d.save("link-timing.svg")

def stopping():
    d=Diagram("05","Three ways to stop","A software Brake request is not a mechanical holding brake.",900)
    for x,label,title,color,lines in [
        (40,"NORMAL STOP","Brake + disarm",BLUE,["Release drive enable.","Or PARK / controller L1.","Rider left: controller brake.","Return neutral; ARM again."]),
        (430,"SOFTWARE E-STOP","Latched Brake",RED,["Rider right / controller B.","Also detected system faults.","Release does not reset.","Repair; restart all programs."]),
        (820,"INDEPENDENT STOP","Remove drive power",AMBER,["Requires reviewed hardware.","Must cover every power path.","Braking changes unpowered.","Prove stopping and holding."])]:
        d.card(x,180,340,245,label,title,lines,color)
    d.text(40,480,"Automatic software trips",25,bold=True)
    d.card(40,510,535,155,"MASTER","Stop both sides",["Rider / radio / steering / input faults.","Lost health replies or stalled control loop."],RED)
    d.card(625,510,535,155,"EACH CHILD","Stop its own motors",["Command lease / invalid packet / local faults.","Heat, speed, battery and duty supervision."],RED)
    d.note(710,"OCCUPIED OPERATION HAS NOT BEEN VALIDATED",["Commands: 40 ms; lease: 150 ms. Health: 100 ms requests + 500 ms reply deadline.","Detection and braking take time. Measure delay and loaded stopping distance on the final chair.","An open rider-button wire or failed drive CPU can defeat software protection."])
    d.save("stop-paths.svg")

if __name__ == "__main__":
    OUT.mkdir(parents=True,exist_ok=True)
    for draw in (topology,ports,operation,node,stopping,signals,timing): draw()
