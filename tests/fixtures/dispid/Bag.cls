VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Bag"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' A class called late, through an Object variable, by name: the LateMem*
' opcodes the late fixture does not already reach (a plain property store,
' named arguments to a Sub, a named indexed store and a named Set).

Private m_Size As Long
Private m_Items(7) As Variant
Private m_Slots(7) As Object

Public Property Get Size() As Long
    Size = m_Size
End Property

Public Property Let Size(ByVal n As Long)
    m_Size = n
End Property

Public Property Get Item(ByVal Index As Long) As Variant
    Item = m_Items(Index)
End Property

Public Property Let Item(ByVal Index As Long, ByVal v As Variant)
    m_Items(Index) = v
End Property

Public Property Get Slot(ByVal Index As Long) As Object
    Set Slot = m_Slots(Index)
End Property

Public Property Set Slot(ByVal Index As Long, ByVal o As Object)
    Set m_Slots(Index) = o
End Property

Public Sub Store(ByVal Key As String, ByVal Value As Variant)
    m_Items(m_Size) = Key & "=" & Value
    m_Size = m_Size + 1
End Sub
