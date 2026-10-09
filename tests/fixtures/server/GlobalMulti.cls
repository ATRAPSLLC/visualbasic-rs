VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "GlobalMulti"
Attribute VB_GlobalNameSpace = True
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' Instancing = 6: a property, a method and an event.

Public Event Changed(ByVal Value As Long)
Private m_Value As Long

Public Property Get Value() As Long
    Value = m_Value
End Property

Public Property Let Value(ByVal v As Long)
    m_Value = v
    RaiseEvent Changed(v)
End Property

Public Function Twice() As Long
    Twice = m_Value * 2
End Function
