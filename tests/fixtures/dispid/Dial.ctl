VERSION 5.00
Begin VB.UserControl Dial 
   ClientHeight    =   600
   ClientLeft      =   0
   ClientTop       =   0
   ClientWidth     =   1500
   ScaleHeight     =   600
   ScaleWidth      =   1500
End
Attribute VB_Name = "Dial"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' A UserControl with one member of each kind. A form reaches the control
' through its extender, which it calls by DISPID: the LateId* opcodes.

Private m_Value As Long
Private m_Target As Object
Private m_Items(7) As String
Private m_Slots(7) As Object

Public Property Get Value() As Long
    Value = m_Value
End Property

Public Property Let Value(ByVal v As Long)
    m_Value = v
End Property

Public Property Get Target() As Object
    Set Target = m_Target
End Property

Public Property Set Target(ByVal o As Object)
    Set m_Target = o
End Property

' An indexed property pair.
Public Property Get Item(ByVal i As Long) As String
    Item = m_Items(i)
End Property

Public Property Let Item(ByVal i As Long, ByVal s As String)
    m_Items(i) = s
End Property

' An indexed object property pair.
Public Property Get Slot(ByVal i As Long) As Object
    Set Slot = m_Slots(i)
End Property

Public Property Set Slot(ByVal i As Long, ByVal o As Object)
    Set m_Slots(i) = o
End Property

Public Sub Spin(ByVal n As Long, Optional ByVal Label As String = "")
    m_Value = m_Value + n
    m_Items(0) = Label
End Sub

Public Sub Clear()
    m_Value = 0
End Sub

Public Function Scaled(ByVal x As Double) As Double
    Scaled = x * m_Value
End Function

Public Function Describe(ByVal prefix As String, Optional ByVal count As Long = 1) As String
    Describe = prefix & String$(count, "*") & CStr(m_Value)
End Function
