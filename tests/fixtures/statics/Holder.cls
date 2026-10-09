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

' A class's Static locals and member variables.

Private m_Hits As Long
Private m_Tag As String

Public Function Touch() As Long
    Static seen As Long
    Static note As String
    seen = seen + 1
    note = note & "t"
    m_Hits = m_Hits + seen
    m_Tag = m_Tag & note
    Touch = m_Hits + Len(m_Tag)
End Function
