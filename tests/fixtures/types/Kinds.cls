VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Kinds"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' One public method per parameter and return type, so each FuncTypDesc type
' code and each call's argument widths can be read against a declaration.

Public Event Fired(ByVal a As Long, ByRef b As String)

Private m_Items(3, 3) As String

Public Function B(ByVal x As Boolean) As Boolean
    B = Not x
End Function

Public Function D(ByVal x As Date) As Date
    D = x + 1
End Function

Public Function I(ByVal x As Integer) As Integer
    I = x + 1
End Function

Public Function L(ByVal x As Long) As Long
    L = x + 1
End Function

Public Function S(ByVal x As Single) As Single
    S = x * 2
End Function

Public Function R(ByVal x As Double) As Double
    R = x * 2
End Function

Public Function C(ByVal x As Currency) As Currency
    C = x * 2
End Function

Public Function T(ByVal x As String) As String
    T = x & "!"
End Function

Public Function V(ByVal x As Variant) As Variant
    V = x
End Function

Public Function O(ByVal x As Object) As Object
    Set O = x
End Function

Public Function Y(ByVal x As Byte) As Byte
    Y = x Xor 1
End Function

Public Function K(ByVal x As Kinds) As Kinds
    Set K = x
End Function

Public Function Refs(ByRef bo As Boolean, ByRef da As Date, ByRef it As Integer, _
        ByRef lo As Long, ByRef si As Single, ByRef db As Double, ByRef cu As Currency, _
        ByRef st As String, ByRef va As Variant, ByRef ob As Object, ByRef by As Byte) As Long
    bo = True
    it = 2
    lo = 3
    Refs = 1
End Function

Public Function Arrays(ByRef al() As Long, ByRef sa() As String, ByRef va() As Variant) As Long()
    Arrays = al
End Function

Public Function Opts(Optional ByVal a As Long = 5, Optional ByVal st As String = "d", _
        Optional ByVal db As Double = 1.5, Optional ByVal bo As Boolean = True, _
        Optional va As Variant) As Long
    If IsMissing(va) Then Opts = a Else Opts = 0
End Function

Public Sub Many(ByVal a As Long, ParamArray rest() As Variant)
    Dim e As Variant
    For Each e In rest
        a = a + 1
    Next
End Sub

Public Property Get Item(ByVal row As Long, ByVal col As Long) As String
    Item = m_Items(row, col)
End Property

Public Property Let Item(ByVal row As Long, ByVal col As Long, ByVal value As String)
    m_Items(row, col) = value
End Property

Friend Function Rec(ByRef p As Pair) As Long
    Rec = p.a + Len(p.b)
End Function

Private Function Hidden(ByVal a As Long, ByVal b As Double) As Currency
    Hidden = a + b
End Function

Public Function UseHidden() As Currency
    UseHidden = Hidden(1, 2.5)
End Function

Public Sub Raise()
    Dim s As String
    s = "e"
    RaiseEvent Fired(1, s)
End Sub
