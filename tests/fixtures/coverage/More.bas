Attribute VB_Name = "More"
Option Explicit

' Objects, Variants and IUnknowns stored through every storage class;
' Variant arrays and conditions; literals; temporaries passed ByRef;
' UDT returns and array assignment; byte arrays.

Private Declare Sub CopyRec Lib "kernel32" Alias "RtlMoveMemory" (Dest As FileRec, Src As FileRec, ByVal n As Long)
Private Declare Sub CopyStrRec Lib "kernel32" Alias "RtlMoveMemory" (Dest As Rec, Src As Rec, ByVal n As Long)
Private mO As Object, mV As Variant, mU As IUnknown, mFx As String * 8

Private Function Make() As Object
    Set Make = New Collection
End Function

Private Function MakeV() As Variant
    Set MakeV = New Collection
End Function

Private Function MakeRec() As FileRec
    MakeRec.n = 1
End Function

Private Function MakeBig() As Rec
    MakeBig.fL = 2
End Function

Private Sub TakeB(b As Byte)
    b = b + 1
End Sub
Private Sub TakeI(i As Integer)
    i = i + 1
End Sub
Private Sub TakeD(d As Double)
    d = d + 1
End Sub
Private Sub TakeS(s As Single)
    s = s + 1
End Sub
Private Sub TakeStr(s As String)
    s = s & "x"
End Sub
Private Sub TakeV(v As Variant)
    v = v + 1
End Sub
Private Sub TakeO(o As Object)
    Set o = Nothing
End Sub
Private Sub TakeR(r As FileRec)
    r.n = r.n + 1
End Sub

Public Function Objects(o As Object, v As Variant, u As IUnknown, ao() As Object, av() As Variant) As Long
    Dim lo As Object, lv As Variant, lu As IUnknown, la(2) As Variant, lao(2) As Object
    Set lo = o: Set o = lo: Set lo = Make(): Set o = Make()
    Set mO = o: Set mO = Make(): Set gO = lo: Set gO = Make()
    Set ao(1) = lo: Set ao(2) = Make(): Set lao(1) = lo: Set lo = ao(1)
    Set lv = o: Set v = o: Set lv = Make(): Set v = Make(): Set mV = o: Set mV = Make()
    Set gV = o: Set gV = Make(): Set av(1) = o: Set av(2) = Make(): Set la(1) = o
    Set lv = MakeV(): Set v = MakeV(): Set mV = MakeV(): Set av(1) = MakeV()
    Set lu = o: Set u = o: Set mU = o: Set lv = lu: Set v = u: Set mV = u: Set av(1) = u: Set la(2) = lu
    Set lv = Nothing: Set u = Nothing
    lv = v: v = lv: mV = lv: lv = mV: gV = v: v = gV: av(1) = v: v = av(1): la(1) = v: lv = la(1)
    If v Then Objects = 1
    If v And lv Then Objects = 2
    Do While lv
        lv = Empty
    Loop
    If v.Count Then Objects = 3
    Dim e As Variant
    For Each e In av
        If e Is Nothing Then Exit For
    Next
    For Each e In v
        Objects = Objects + 1
    Next
    Mid(lv, 1, 1) = "x"
    MidB(lv, 1, 2) = "y"
    lv = 1.5@: lv = #1/1/2000#: lv = 1.5!: lv = CByte(7): lv = 2.5: lv = "s": lv = True: lv = 300: lv = 70000
    lv = 2 ^ 3: lv = lv ^ 2
End Function

Public Function Temps() As Double
    Dim d As Double, b As Byte, i As Integer, s As Single, st As String, v As Variant, r As FileRec, big As Rec, o As Object
    TakeB 1: TakeI 2: TakeD 3.5: TakeS 1.5!: TakeStr "a": TakeV 4: TakeO Nothing
    TakeB b + 1: TakeI i * 2: TakeD d / 2: TakeS s + 1: TakeStr st & "b": TakeV v + 1
    TakeB (b): TakeI (i): TakeD (d): TakeStr (st): TakeV (v): TakeR MakeRec()
    r = MakeRec(): big = MakeBig(): CopyRec r, r, 8: CopyStrRec big, big, 8
    Dim ra() As FileRec, rb() As FileRec, ba() As Byte, bs As String
    ReDim ra(2): rb = ra: ra = rb
    ba = "abc": bs = ba: v = ba: ba = v: bs = StrConv(ba, vbUnicode)
    mFx = "fixed": st = mFx: LSet mFx = "l": RSet mFx = "r"
    Dim fx As String * 4
    fx = st: st = fx: fx = mFx
    d = d ^ i: d = 2 ^ i
    Line Input #1, v
    Temps = d
End Function

