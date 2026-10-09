VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Holder"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' Members of every type used in its own code, and its own typed functions
' called through Me.

Private pB As Byte
Private pI As Integer
Private pL As Long
Private pS As Single
Private pD As Double
Private pC As Currency
Private pT As Date
Private pStr As String
Private pV As Variant
Private pF As Boolean
Private pO As Object

Public Function GetB() As Byte
    GetB = pB
End Function

Public Sub SetB(ByVal x As Byte)
    pB = x
End Sub

Public Function GetI() As Integer
    GetI = pI
End Function

Public Sub SetI(ByVal x As Integer)
    pI = x
End Sub

Public Function GetL() As Long
    GetL = pL
End Function

Public Sub SetL(ByVal x As Long)
    pL = x
End Sub

Public Function GetS() As Single
    GetS = pS
End Function

Public Sub SetS(ByVal x As Single)
    pS = x
End Sub

Public Function GetD() As Double
    GetD = pD
End Function

Public Sub SetD(ByVal x As Double)
    pD = x
End Sub

Public Function GetC() As Currency
    GetC = pC
End Function

Public Sub SetC(ByVal x As Currency)
    pC = x
End Sub

Public Function GetT() As Date
    GetT = pT
End Function

Public Sub SetT(ByVal x As Date)
    pT = x
End Sub

Public Function GetStr() As String
    GetStr = pStr
End Function

Public Sub SetStr(ByVal x As String)
    pStr = x
End Sub

Public Function GetV() As Variant
    GetV = pV
End Function

Public Sub SetV(ByVal x As Variant)
    pV = x
End Sub

Public Function GetF() As Boolean
    GetF = pF
End Function

Public Sub SetF(ByVal x As Boolean)
    pF = x
End Sub

Public Function Round() As Double
    Dim d As Double
    SetB 3
    pB = Me.GetB()
    d = d + Me.GetB() + pB
    SetI 300
    pI = Me.GetI()
    d = d + Me.GetI() + pI
    SetL 70000
    pL = Me.GetL()
    d = d + Me.GetL() + pL
    SetS 1.5
    pS = Me.GetS()
    d = d + Me.GetS() + pS
    SetD 2.25
    pD = Me.GetD()
    d = d + Me.GetD() + pD
    SetC 3.5@
    pC = Me.GetC()
    d = d + Me.GetC() + pC
    SetT #1/2/2003#
    pT = Me.GetT()
    SetStr "ab"
    pStr = Me.GetStr()
    SetV 7
    pV = Me.GetV()
    d = d + Me.GetV() + pV
    SetF True
    pF = Me.GetF()
    Set pO = Me
    Round = d
End Function

