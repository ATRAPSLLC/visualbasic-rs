VERSION 5.00
Begin VB.Form Canvas 
   Caption         =   "Canvas"
   ClientHeight    =   3090
   ClientLeft      =   60
   ClientTop       =   450
   ClientWidth     =   4680
   LinkTopic       =   "Canvas"
   ScaleHeight     =   3090
   ScaleWidth      =   4680
End
Attribute VB_Name = "Canvas"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

' Print on the form and on a picture of it, every item separator.

Public Sub Draw(ByVal i As Integer, ByVal s As String, ByVal v As Variant)
    Print i; s, v
    Print
    Print Spc(2); i; Tab(5); s;
    Print i,
    Me.Print s
    Me.Print v; i
    Debug.Print i; s
    Printer.Print s
End Sub

