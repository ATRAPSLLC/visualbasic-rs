VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Ring"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' Implements Measure; its own private methods of each return type are called
' through Me (ThisVCall*), and a Friend method from the module.

Implements Measure

Private m_Radius As Single

Private Sub Class_Initialize()
    m_Radius = 1
End Sub

Private Sub Class_Terminate()
    m_Radius = 0
End Sub

Public Property Let Radius(ByVal r As Single)
    m_Radius = r
End Property

Public Property Get Radius() As Single
    Radius = m_Radius
End Property

Friend Function Diameter() As Double
    Diameter = m_Radius * 2
End Function

Private Function PiR4() As Single
    PiR4 = 3.14159
End Function

Private Function PiR8() As Double
    PiR8 = 3.14159265358979
End Function

Private Function PiCy() As Currency
    PiCy = 3.1416@
End Function

Private Function Today() As Date
    Today = #1/2/2000#
End Function

Private Function Count() As Byte
    Count = 1
End Function

Private Function Small() As Integer
    Small = 2
End Function

Private Function Big() As Long
    Big = 3
End Function

Private Function Text() As String
    Text = "ring"
End Function

Private Function Anything() As Variant
    Anything = m_Radius
End Function

Private Function Self() As Ring
    Set Self = Me
End Function

Private Function Flag() As Boolean
    Flag = m_Radius > 0
End Function

Private Function Measure_Area() As Single
    Measure_Area = PiR4() * m_Radius * m_Radius
End Function

Private Function Measure_Perimeter() As Double
    Measure_Perimeter = 2 * PiR8() * m_Radius
End Function

Private Function Measure_Cost() As Currency
    Measure_Cost = PiCy() * Count()
End Function

Private Function Measure_Made() As Date
    Measure_Made = Today()
End Function

Private Function Measure_Sides() As Byte
    Measure_Sides = Count() - 1
End Function

Private Function Measure_Label() As String
    Measure_Label = Text() & CStr(Small() + Big())
End Function

Private Function Measure_Weight() As Variant
    Measure_Weight = Anything()
End Function

Private Property Get Measure_Valid() As Boolean
    Measure_Valid = Flag() And Not (Self() Is Nothing)
End Property

Private Sub Measure_Resize(ByVal factor As Single)
    m_Radius = m_Radius * factor
End Sub
