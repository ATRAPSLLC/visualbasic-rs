VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Counter"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

Public Event Changed(ByVal value As Long)

Private m_Value As Long
Private m_Scale As Double
Private m_Owner As Object

Public Property Get Value() As Long
    Value = m_Value
End Property

Public Property Let Value(ByVal v As Long)
    m_Value = v
    RaiseEvent Changed(v)
End Property

Public Property Set Owner(ByVal o As Object)
    Set m_Owner = o
End Property

Public Function AddLong(ByVal a As Long, ByVal b As Long) As Long
    AddLong = a + b + Bump(1)
End Function

Public Function ScaleBy(ByVal factor As Double) As Double
    m_Scale = m_Scale * factor
    ScaleBy = m_Scale
End Function

Public Function Money(ByVal c As Currency) As Currency
    Money = c * 2
End Function

Public Function Describe(ByVal v As Variant, ByRef text As String) As Variant
    text = text & CStr(v)
    Describe = v
End Function

Public Function Pick(Optional ByVal which As Integer = 1) As Integer
    Pick = which + 1
End Function

Public Function Total(ParamArray values() As Variant) As Long
    Dim i As Long
    For i = LBound(values) To UBound(values)
        Total = Total + values(i)
    Next
End Function

Public Function Ratio(ByVal a As Single) As Single
    Ratio = a / 2
End Function

Public Function Small(ByVal a As Byte) As Byte
    Small = a + 1
End Function

Public Function Self() As Counter
    Set Self = Me
End Function

Private Function Bump(ByVal n As Long) As Long
    Bump = n + m_Value
End Function

Public Function Twice() As Long
    Twice = Me.AddLong(m_Value, m_Value) + Bump(2)
End Function
