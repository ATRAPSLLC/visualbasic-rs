VERSION 5.00
Begin VB.MDIForm Frame 
   BackColor       =   &H8000000C&
   Caption         =   "Frame"
   ClientHeight    =   3195
   ClientLeft      =   60
   ClientTop       =   345
   ClientWidth     =   4680
   LinkTopic       =   "Frame"
   StartUpPosition =   3  'Windows Default
End
Attribute VB_Name = "Frame"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

' An MDIForm: its own public members, calls on its built-in members
' (Arrange, Caption, ActiveForm), and child forms held in variables.

Private m_Opened As Long

Public Sub OpenChild(ByVal Title As String)
    Dim c As Child
    Set c = New Child
    c.Reset
    c.Caption = Title
    c.Show
    m_Opened = m_Opened + 1
    Tile
End Sub

Public Property Get Opened() As Long
    Opened = m_Opened
End Property

Public Sub Tile()
    Me.Arrange 1
End Sub

Private Function ActiveTotal() As Long
    Dim f As Child
    If Not Me.ActiveForm Is Nothing Then
        Set f = Me.ActiveForm
        ActiveTotal = f.Total
    End If
End Function

Private Sub MDIForm_Load()
    Me.Caption = "Frame " & m_Opened
    OpenChild "first"
    OpenChild "second"
    m_Opened = m_Opened + ActiveTotal()
End Sub

Private Sub MDIForm_Unload(Cancel As Integer)
    m_Opened = 0
End Sub
