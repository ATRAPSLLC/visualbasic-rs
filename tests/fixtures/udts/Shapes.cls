VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Shapes"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' Public and Private Types of a class: a nested record, an array of
' records, an object and a Double member, records passed and returned, and
' a module's Types used in a class.

Public Type Point
    X As Long
    Y As Long
End Type

Public Type Named
    Name As String
    At As Point
    Tags() As String
    V As Variant
End Type

Public Type Third
    O As Object
    D As Double
End Type

Private Type PrivT
    S As String
    Z As Long
End Type

Private Type Holder
    Ns() As Named
    Q As String
End Type

Public Function Local1() As String
    Dim n As Named, p As PrivT, h As Holder, m As ModT
    n.Name = "a"
    p.S = "b"
    m.T = "c"
    ReDim h.Ns(1)
    Open "x" For Random As #1 Len = 100
    Put #1, , h
    Close #1
    Local1 = n.Name & p.S & m.T
End Function

Public Function Make(ByVal X As Long) As Point
    Make.X = X
End Function

Public Function Name(n As Named) As String
    Name = n.Name
End Function

Public Function Thr(t As Third) As Double
    Thr = t.D
End Function
