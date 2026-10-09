Attribute VB_Name = "TypedOps"
Option Explicit

' Per type: arithmetic, comparisons, logic, and loads and stores through
' every storage class: locals, ByRef parameters, the module's own
' variables, another module's, array elements, UDT fields, With blocks.

Private mB As Byte
Private mI As Integer
Private mL As Long
Private mS As Single
Private mD As Double
Private mC As Currency
Private mT As Date
Private mStr As String
Private mV As Variant
Private mF As Boolean
Private mR As Rec

Public Function OpB(ByVal a As Byte, ByVal b As Byte, r As Byte, arr() As Byte, rr As Rec) As Byte
    Dim x As Byte, y As Byte, ok As Boolean, k As Long, la(3) As Byte
    x = a + b
    y = a - b
    x = x * y
    y = -a
    x = a \ b
    x = a Mod b
    y = a And b
    y = a Or b
    y = a Xor b
    y = a Eqv b
    y = a Imp b
    y = Not a
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mB = x
    y = mB
    gB = y
    x = gB
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fB = x
    y = rr.fB
    mR.fB = y
    x = mR.fB
    gR.fB = x
    y = gR.fB
    With rr
        .fB = y
        x = .fB
    End With
    With mR
        .fB = x
        y = .fB
    End With
    With gR
        .fB = y
        x = .fB
    End With
    Select Case x
        Case a To b: k = 1
        Case Is < b: k = 2
    End Select
    For x = a To b Step 1
        k = k + 1
    Next
    For y = a To b
        k = k + 1
    Next
    OpB = x + y
End Function

Public Function OpI(ByVal a As Integer, ByVal b As Integer, r As Integer, arr() As Integer, rr As Rec) As Integer
    Dim x As Integer, y As Integer, ok As Boolean, k As Long, la(3) As Integer
    x = a + b
    y = a - b
    x = x * y
    y = -a
    y = Abs(a) + Sgn(b)
    x = a \ b
    x = a Mod b
    y = a And b
    y = a Or b
    y = a Xor b
    y = a Eqv b
    y = a Imp b
    y = Not a
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mI = x
    y = mI
    gI = y
    x = gI
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fI = x
    y = rr.fI
    mR.fI = y
    x = mR.fI
    gR.fI = x
    y = gR.fI
    With rr
        .fI = y
        x = .fI
    End With
    With mR
        .fI = x
        y = .fI
    End With
    With gR
        .fI = y
        x = .fI
    End With
    Select Case x
        Case a To b: k = 1
        Case Is < b: k = 2
    End Select
    For x = a To b Step 1
        k = k + 1
    Next
    For y = a To b
        k = k + 1
    Next
    OpI = x + y
End Function

Public Function OpL(ByVal a As Long, ByVal b As Long, r As Long, arr() As Long, rr As Rec) As Long
    Dim x As Long, y As Long, ok As Boolean, k As Long, la(3) As Long
    x = a + b
    y = a - b
    x = x * y
    y = -a
    y = Abs(a) + Sgn(b)
    x = a \ b
    x = a Mod b
    y = a And b
    y = a Or b
    y = a Xor b
    y = a Eqv b
    y = a Imp b
    y = Not a
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mL = x
    y = mL
    gL = y
    x = gL
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fL = x
    y = rr.fL
    mR.fL = y
    x = mR.fL
    gR.fL = x
    y = gR.fL
    With rr
        .fL = y
        x = .fL
    End With
    With mR
        .fL = x
        y = .fL
    End With
    With gR
        .fL = y
        x = .fL
    End With
    Select Case x
        Case a To b: k = 1
        Case Is < b: k = 2
    End Select
    For x = a To b Step 1
        k = k + 1
    Next
    For y = a To b
        k = k + 1
    Next
    OpL = x + y
End Function

Public Function OpS(ByVal a As Single, ByVal b As Single, r As Single, arr() As Single, rr As Rec) As Single
    Dim x As Single, y As Single, ok As Boolean, k As Long, la(3) As Single
    x = a + b
    y = a - b
    x = x * y
    y = -a
    y = Abs(a) + Sgn(b)
    x = a / b
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mS = x
    y = mS
    gS = y
    x = gS
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fS = x
    y = rr.fS
    mR.fS = y
    x = mR.fS
    gR.fS = x
    y = gR.fS
    With rr
        .fS = y
        x = .fS
    End With
    With mR
        .fS = x
        y = .fS
    End With
    With gR
        .fS = y
        x = .fS
    End With
    Select Case x
        Case a To b: k = 1
        Case Is < b: k = 2
    End Select
    For x = a To b Step 1
        k = k + 1
    Next
    For y = a To b
        k = k + 1
    Next
    OpS = x + y
End Function

Public Function OpD(ByVal a As Double, ByVal b As Double, r As Double, arr() As Double, rr As Rec) As Double
    Dim x As Double, y As Double, ok As Boolean, k As Long, la(3) As Double
    x = a + b
    y = a - b
    x = x * y
    y = -a
    y = Abs(a) + Sgn(b)
    x = a / b
    y = a ^ 2
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mD = x
    y = mD
    gD = y
    x = gD
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fD = x
    y = rr.fD
    mR.fD = y
    x = mR.fD
    gR.fD = x
    y = gR.fD
    With rr
        .fD = y
        x = .fD
    End With
    With mR
        .fD = x
        y = .fD
    End With
    With gR
        .fD = y
        x = .fD
    End With
    Select Case x
        Case a To b: k = 1
        Case Is < b: k = 2
    End Select
    For x = a To b Step 1
        k = k + 1
    Next
    For y = a To b
        k = k + 1
    Next
    OpD = x + y
End Function

Public Function OpC(ByVal a As Currency, ByVal b As Currency, r As Currency, arr() As Currency, rr As Rec) As Currency
    Dim x As Currency, y As Currency, ok As Boolean, k As Long, la(3) As Currency
    x = a + b
    y = a - b
    x = x * y
    y = -a
    y = Abs(a) + Sgn(b)
    x = a / b
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mC = x
    y = mC
    gC = y
    x = gC
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fC = x
    y = rr.fC
    mR.fC = y
    x = mR.fC
    gR.fC = x
    y = gR.fC
    With rr
        .fC = y
        x = .fC
    End With
    With mR
        .fC = x
        y = .fC
    End With
    With gR
        .fC = y
        x = .fC
    End With
    Select Case x
        Case a To b: k = 1
        Case Is < b: k = 2
    End Select
    For x = a To b Step 1
        k = k + 1
    Next
    For y = a To b
        k = k + 1
    Next
    OpC = x + y
End Function

Public Function OpT(ByVal a As Date, ByVal b As Date, r As Date, arr() As Date, rr As Rec) As Date
    Dim x As Date, y As Date, ok As Boolean, k As Long, la(3) As Date
    x = a + 1
    y = b - 1
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mT = x
    y = mT
    gT = y
    x = gT
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fT = x
    y = rr.fT
    mR.fT = y
    x = mR.fT
    gR.fT = x
    y = gR.fT
    With rr
        .fT = y
        x = .fT
    End With
    With mR
        .fT = x
        y = .fT
    End With
    With gR
        .fT = y
        x = .fT
    End With
    OpT = x
End Function

Public Function OpStr(ByVal a As String, ByVal b As String, r As String, arr() As String, rr As Rec) As String
    Dim x As String, y As String, ok As Boolean, k As Long, la(3) As String
    x = a & b
    y = Mid$(a, 1, 1) & Left$(b, 1)
    k = Len(a) + LenB(a) + InStr(a, b) + InStrB(a, b) + StrComp(a, b)
    Mid$(x, 1, 1) = "z"
    MidB$(x, 1, 2) = "y"
    ok = a Like b
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mStr = x
    y = mStr
    gStr = y
    x = gStr
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fStr = x
    y = rr.fStr
    mR.fStr = y
    x = mR.fStr
    gR.fStr = x
    y = gR.fStr
    With rr
        .fStr = y
        x = .fStr
    End With
    With mR
        .fStr = x
        y = .fStr
    End With
    With gR
        .fStr = y
        x = .fStr
    End With
    OpStr = x
End Function

Public Function OpV(ByVal a As Variant, ByVal b As Variant, r As Variant, arr() As Variant, rr As Rec) As Variant
    Dim x As Variant, y As Variant, ok As Boolean, k As Long, la(3) As Variant
    x = a + b
    y = a - b
    x = x * y
    y = -a
    y = Abs(a) + Sgn(b)
    x = a / b
    x = a Mod b
    y = a And b
    y = a Or b
    y = a Xor b
    y = a Eqv b
    y = a Imp b
    y = Not a
    y = a ^ 2
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    If a Then k = 1
    If a = b Then k = 2
    r = x
    y = r
    mV = x
    y = mV
    gV = y
    x = gV
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fV = x
    y = rr.fV
    mR.fV = y
    x = mR.fV
    gR.fV = x
    y = gR.fV
    With rr
        .fV = y
        x = .fV
    End With
    With mR
        .fV = x
        y = .fV
    End With
    With gR
        .fV = y
        x = .fV
    End With
    OpV = x + y
End Function

Public Function OpF(ByVal a As Boolean, ByVal b As Boolean, r As Boolean, arr() As Boolean, rr As Rec) As Boolean
    Dim x As Boolean, y As Boolean, ok As Boolean, k As Long, la(3) As Boolean
    x = a And b
    y = Not a
    x = a Or b
    y = a Xor b
    x = a Eqv b
    y = a Imp b
    ok = (a = b) Or (a <> b) Or (a < b) Or (a <= b) Or (a > b) Or (a >= b)
    r = x
    y = r
    mF = x
    y = mF
    gF = y
    x = gF
    arr(1) = x
    y = arr(1)
    la(2) = y
    x = la(2)
    rr.fF = x
    y = rr.fF
    mR.fF = y
    x = mR.fF
    gR.fF = x
    y = gR.fF
    With rr
        .fF = y
        x = .fF
    End With
    With mR
        .fF = x
        y = .fF
    End With
    With gR
        .fF = y
        x = .fF
    End With
    OpF = x
End Function

