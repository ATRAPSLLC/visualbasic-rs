VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Watcher"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' WithEvents variables of a project class and of a dispinterface event
' source (VB's TextBox), and members typed by a VB control.

Public WithEvents Src As Shapes
Private WithEvents Txt As VB.TextBox

Public Sub Watch(ByVal t As VB.TextBox, ByVal f As StdFont)
    Set Txt = t
End Sub

Public Function Box() As VB.TextBox
    Set Box = Txt
End Function

Private Sub Src_Moved(ByVal Where As Collection)
    Debug.Print Where.Count
End Sub

Private Sub Txt_Change()
    Debug.Print Txt.Text
End Sub
