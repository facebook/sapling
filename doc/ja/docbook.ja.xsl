<?xml version='1.0' encoding="UTF-8"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version='1.0'>
  <xsl:import href="http://docbook.sourceforge.net/release/xsl/current/manpages/docbook.xsl"/>
  <xsl:output encoding="UTF-8"/>

  <xsl:template match="refnamediv">
  <xsl:text>.SH 名前&#10;</xsl:text>
  <xsl:for-each select="refname">
    <xsl:if test="position()>1">
      <xsl:text>, </xsl:text>
    </xsl:if>
    <xsl:value-of select="."/>
  </xsl:for-each>
  <xsl:text> \- </xsl:text>
  <xsl:value-of select="normalize-space (refpurpose)"/>
  </xsl:template>

  <xsl:template match="refsynopsisdiv">
  <xsl:text>&#10;.SH "書式"&#10;</xsl:text>
  <xsl:apply-templates/>
  </xsl:template>

</xsl:stylesheet>
